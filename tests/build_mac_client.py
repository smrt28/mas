#!/usr/bin/env python3
"""Exercise macOS packaging with fake toolchains; no Windows or Apple build runs."""
import json
import os
from pathlib import Path
import plistlib
import shutil
import subprocess
import tempfile
import unittest
import zipfile

SCRIPT = Path(__file__).resolve().parents[1] / 'tools/build_mac_client.sh'
MOCK = r'''#!/usr/bin/env python3
import json, os, sys, zipfile
from pathlib import Path
name = Path(sys.argv[0]).name
args = sys.argv[1:]
with open(os.environ['MAC_TEST_TRACE'], 'a') as f:
    f.write(json.dumps([name, args]) + '\n')
root = Path(os.environ['MAC_TEST_PROJECT'])
sdk = Path(os.environ['MAC_TEST_SDK'])
def archive(app, output):
    with zipfile.ZipFile(output, 'w') as z:
        for path in app.rglob('*'):
            z.write(path, str(Path(app.name) / path.relative_to(app)))
if name == 'uname':
    print(os.environ['MAC_TEST_SYSTEM'])
elif name == 'rustc':
    print(os.environ['MAC_TEST_STD'])
elif name == 'rustup':
    assert args == ['target', 'add', 'aarch64-apple-darwin']
    Path(os.environ['MAC_TEST_STD']).mkdir()
elif name == 'cargo':
    if args[0] == 'pkgid':
        print('path+file://' + str(root) + '#marsal-a-spion@0.1.0')
    else:
        assert args[0] == 'build'
        assert args[args.index('--target') + 1] == 'aarch64-apple-darwin'
        assert args[args.index('--bin') + 1] == 'mas-client'
        assert args[args.index('--features') + 1] == 'client'
        assert '--no-default-features' in args and '--locked' in args and '--release' in args
        assert os.environ['MACOSX_DEPLOYMENT_TARGET'] == '11.0'
        assert os.environ['SDKROOT'] == str(sdk)
        assert '-arch arm64' in os.environ['CFLAGS_aarch64_apple_darwin']
        assert os.environ['CC_SHELL_ESCAPED_FLAGS'] == '1'
        out = Path(args[args.index('--target-dir') + 1]) / 'aarch64-apple-darwin/release/mas-client'
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_bytes(b'MOCK ARM64 EXECUTABLE: embedded graphics and key')
elif name == 'xcrun':
    if '--show-sdk-path' in args:
        print(sdk)
    else:
        print(Path(os.environ['MAC_TEST_BIN']) / ('apple-' + args[-1]))
elif name in ('codesign', 'rcodesign'):
    path = Path(args[-1])
    assert path.exists()
    if path.is_dir() and '--verify' not in args:
        signature = path / 'Contents/_CodeSignature'
        signature.mkdir(exist_ok=True)
        (signature / 'CodeResources').write_text('mock ad-hoc signature')
elif name == 'ditto':
    archive(Path(args[-2]), Path(args[-1]))
elif name == 'zip':
    archive(Path(args[-1]), Path(args[-2]))
'''


class MacBuildScript(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='mas mac script ')
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.project = self.root / 'project with spaces'
        (self.project / 'tools').mkdir(parents=True)
        shutil.copy2(SCRIPT, self.project / 'tools/build_mac_client.sh')
        self.bin = self.root / 'fake tools'
        self.bin.mkdir()
        for name in ('uname', 'rustc', 'rustup', 'cargo', 'xcrun', 'apple-clang',
                     'apple-ar', 'codesign', 'ditto', 'rcodesign', 'zip'):
            self.tool(self.bin / name)
        self.toolchain = self.root / 'cross checkout/target'
        (self.toolchain / 'bin').mkdir(parents=True)
        for name in ('arm64-apple-darwin23.5-clang', 'arm64-apple-darwin23.5-ar'):
            self.tool(self.toolchain / 'bin' / name)
        self.sdk = self.toolchain / 'SDK/MacOSX14.5.sdk'
        self.sdk.mkdir(parents=True)
        self.trace = self.root / 'trace.jsonl'
        self.env = os.environ.copy()
        for name in ('OSXCROSS_ROOT', 'SDKROOT', 'MACOSX_DEPLOYMENT_TARGET', 'CARGO_TARGET_DIR'):
            self.env.pop(name, None)
        self.env.update(PATH=str(self.bin) + os.pathsep + self.env['PATH'],
                        MAC_TEST_TRACE=str(self.trace), MAC_TEST_PROJECT=str(self.project),
                        MAC_TEST_SDK=str(self.sdk), MAC_TEST_BIN=str(self.bin),
                        MAC_TEST_STD=str(self.root / 'rust std'), MAC_TEST_SYSTEM='Darwin')

    @staticmethod
    def tool(path):
        path.write_text(MOCK)
        path.chmod(0o755)

    def run_script(self):
        return subprocess.run(['bash', str(self.project / 'tools/build_mac_client.sh')],
                              cwd=self.root, env=self.env, capture_output=True, text=True)

    def calls(self):
        return [json.loads(line) for line in self.trace.read_text().splitlines()]

    def check_package(self):
        output = self.project / 'target/mac'
        binary = output / 'mas-client'
        app = output / 'Marsal a spion.app'
        self.assertTrue(os.access(binary, os.X_OK))
        self.assertEqual(binary.read_bytes(), (app / 'Contents/MacOS/mas-client').read_bytes())
        with (app / 'Contents/Info.plist').open('rb') as source:
            info = plistlib.load(source)
        self.assertEqual(info['CFBundleExecutable'], 'mas-client')
        self.assertEqual(info['CFBundleShortVersionString'], '0.1.0')
        self.assertEqual(info['LSMinimumSystemVersion'], '11.0')
        self.assertTrue(info['NSHighResolutionCapable'])
        with zipfile.ZipFile(output / 'mas-client-macos-arm64.zip') as archive:
            names = archive.namelist()
            self.assertIn('Marsal a spion.app/Contents/MacOS/mas-client', names)
            self.assertIn('Marsal a spion.app/Contents/_CodeSignature/CodeResources', names)
            executable = archive.getinfo('Marsal a spion.app/Contents/MacOS/mas-client')
            self.assertTrue((executable.external_attr >> 16) & 0o111)
            self.assertFalse(any('/assets/' in name or '.key' in name for name in names))
        self.assertFalse(list(output.glob('.package.*')))

    def test_native_packaging_target_install_and_rebuild_cleanup(self):
        result = self.run_script()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.check_package()
        output = self.project / 'target/mac'
        (output / 'Marsal a spion.app/stale-file').write_text('old generated bundle')
        (output / 'keep.txt').write_text('unrelated artifact')
        result = self.run_script()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.check_package()
        self.assertFalse((output / 'Marsal a spion.app/stale-file').exists())
        self.assertTrue((output / 'keep.txt').exists())
        self.assertEqual(sum(name == 'rustup' for name, _ in self.calls()), 1)
        self.assertTrue(any(name == 'codesign' and '--verify' in args for name, args in self.calls()))

    def test_linux_cross_packaging_with_custom_cargo_directory_and_checkout_root(self):
        self.env.update(MAC_TEST_SYSTEM='Linux', OSXCROSS_ROOT=str(self.toolchain.parent),
                        CARGO_TARGET_DIR=str(self.root / 'custom cargo output'))
        result = self.run_script()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.check_package()
        self.assertTrue(any(name == 'rcodesign' for name, _ in self.calls()))
        self.assertFalse(any(name in ('codesign', 'xcrun') for name, _ in self.calls()))

    def test_missing_cross_toolchain_fails_before_downloading_or_building(self):
        self.env['MAC_TEST_SYSTEM'] = 'Linux'
        result = self.run_script()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('OSXCROSS_ROOT', result.stderr)
        self.assertFalse(any(name in ('cargo', 'rustup') for name, _ in self.calls()))

    def test_deployment_target_before_apple_silicon_is_rejected(self):
        self.env['MACOSX_DEPLOYMENT_TARGET'] = '10.15'
        result = self.run_script()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('11.0 or newer', result.stderr)


if __name__ == '__main__':
    unittest.main()
