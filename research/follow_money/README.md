# Follow the money (round 3)

Scripts that measure who actually profits on pump.fun, from the binary tape written by
`lab tape --data <store> --out <dir>` (trades, creates, completes, PumpSwap pool events).

| script | question |
|---|---|
| `costbasis.py` | do sells cluster at preset take-profit / stop-loss multiples of each wallet's entry? |
| `winners.py` | per-wallet PnL after curve fees, priority fees and tips; PnL by archetype |
| `consistent.py` | wallets profitable in every period |
| `dive.py`, `exits.py` | position-level behaviour of the consistent takers |
| `selection.py` | forward returns after their entries vs everyone else's, matched on age and curve level |
| `statefeat.py` | on-chain state at every first buy (what the cohort selects on) |
| `oracle.py` | upper bound: enter where the cohort entered, with our landing and costs |
| `b0rank.py` | create-slot buyers' ROI by position after the create transaction |
| `mayhem_money.py` | the mayhem agent and its counterparties |
| `boost.py`, `boostpath.py`, `boostcheck.py` | PumpSwap boost buy-and-burn crank: cadence, spend rule, impact vs fees, front-run test |
| `gradboost.py`, `graduation.py` | graduation: boost program per coin, ride-the-boost simulation |
| `gap1.py` | who occupies the transaction right after a create |

Run from this directory with the tape directories as arguments, e.g.
`python costbasis.py ../../tape/pilot`. Findings are in `docs/ROUND3.md`.
