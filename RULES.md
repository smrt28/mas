# Rules from the Czech PDF

Source: `mas.pdf`, rules on pages 4–5; printed inventory/artwork on pages 7–10. The complete plan and sharper piece artwork are supplied in `tmp/mas-plan.pdf`. Pages 1–3 of `mas.pdf` introduce the game; page 6 describes assembling the physical box and folding pieces.

## Army

Each commander has 40 pieces, placed in any arrangement in their four back rows, faces hidden from the opponent.

| Rank (strongest first) | Czech | Count | Special rule |
|---|---|---:|---|
| Marshal | Maršál | 1 | Beats every ordinary rank except the spy |
| General | Generál | 1 | Ordinary rank |
| Captain | Kapitán | 2 | Ordinary rank |
| Cadet | Kadet | 3 | Ordinary rank |
| Corporal | Kaprál | 4 | Ordinary rank |
| Rifleman | Střelec | 5 | Ordinary rank |
| Scout | Průzkumník | 8 | Can travel farther orthogonally |
| Miner | Minér | 6 | Only rank that clears a mine |
| Spy | Špión | 1 | Beats the marshal; loses to other unequal mobile ranks |
| Mine | Minové pole | 8 | Cannot move; destroys attackers except miners |
| Flag | Prapor | 1 | Cannot move; its capture loses the game |

## Movement and fighting

All mobile pieces move one square forward, backward, sideways **or diagonally**. Pieces cannot jump over other pieces. Scouts additionally move any number of squares on a clear orthogonal line, stopping at an enemy if attacking. Scouts do not have long diagonal moves. Mines and flags cannot move.

An attack occupies an opponent's square and must be **orthogonal**, never diagonal. The attacker privately inspects the defending rank. The attacker's rank is not announced to the defender. If the attacker is stronger, it takes the defender's square; if weaker, it is removed and the defender stays. Equal ranks remove both. Captured pieces leave the board permanently.

The spy/marshal exception is symmetric: the Czech text says the spy can be captured by all moving pieces except the marshal, and that the marshal can be captured only by the spy. Thus a defending spy also defeats an attacking marshal. A spy-versus-spy fight removes both.

A mine stays in place after defeating an attacker. A miner defeats the mine and occupies its square. Any moving piece captures a flag. Neither side can enter either of the two central areas without a grid.

The player whose flag is captured loses. At the end of a game, both armies’ remaining pieces are revealed. After flag capture, the winner sees “You win!” and the loser sees “You lose!” in bold in the top bar; OK returns to the main screen. The text also allows a draw when neither side can hope to capture a flag, and specifically describes losing all remaining mobile pieces as a way to obtain a stalemate draw.

## Decisions for the digital edition

The board geometry follows the complete plan; the remaining decisions specify the timed network game:

1. **Board geometry:** 10 columns × 11 rows, exactly as shown in `tmp/mas-plan.pdf`. The two blocked 1×3 flag panels occupy D5/D6/D7 and G5/G6/G7. Each army deploys in its four back rows (ranks 1–4 or 8–11), leaving three neutral rows in the middle (ranks 5–7). The printed flags in the gaps are board decoration, not capturable pieces.
2. **Diagonal corners:** a one-square diagonal move between blocked squares is allowed when its destination is free. Only a scout's multi-square orthogonal path is checked for intervening obstacles. No jumping is allowed.
3. **First move:** the server randomly chooses the starting commander after deployment. There are no extra scout/spy abilities or dice.
4. **Setup clock:** both players share a clock beginning when the second joins. The client automatically locks a player’s deployment after all 40 pieces are placed; play begins immediately once both are ready. At expiry, unfilled squares receive a shuffled, exact remaining inventory, preserving existing placement.
5. **Move clock:** time expiry passes the turn. A configurable number of consecutive personal misses loses the game (default three). A valid move clears that commander's miss counter. Invalid actions do not extend time. Each next turn gets a fresh full allowance.
6. **Draw adjudication:** no legal moves for either commander ends the match as a draw, even if caused by a blocked army rather than the complete loss of mobile pieces. A configurable number of moves without any piece being captured also draws (default 200). This provides a finite resolution to repetition and inaccessible-flag positions; it does not attempt a strategic proof of whether a flag can ever be captured.
7. **Knowledge:** only the attacker gets the defending rank in a private field report. Captured ranks are recorded for both players. The rank of a surviving attacker is not revealed. All surviving ranks are revealed at match end. Scouts can naturally be inferred from long moves.
8. **No reconnection:** a disconnect interrupts the game without a winner; the other player sees “Broken pike” and returns to the main screen after OK. Resignations lose immediately. A disconnect does not change an already final result. A third connection cannot replace a player. The last departure removes the room; restarting the server removes all rooms.
9. **No redeployment:** locked deployment cannot be edited. Before automatic locking, right-clicking a placed piece returns it to its printed bottom-strip reserve. At timeout, automatic filling preserves pieces already placed. Placement does not reveal enemy occupancy or readiness details beyond the shared ready indicators.

The rule engine, coordinator and protocol tests cover these decisions. The supplied PDF remains the source artwork; extraction adds no generated pieces. The interface uses English instructions and the original Czech rank names.
