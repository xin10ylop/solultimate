# Frozen before the holdout (decided on data/pilot only, 2026-11-01)

Exit for all: `x_tpnone_sl0.15_tr0.2-0.1_h15` (no fixed TP, trailing stop arms at +20% and
exits on a 10-point giveback, stop loss −15%, max hold 15 s, idle exit 15 s).

| id | signal | extra filter |
|---|---|---|
| A | `rm_b0_s0.3_f0.5_w7200_n2`: at first-candle close, create-slot holders ≥ 30% of supply, ≥ 50% of create-slot buyers freshly funded (≤ 2 h), ≥ 2 buyers | none |
| B | `rm_g0_s0.5_f0.5_w21600_n2`: at the end of the creator's bundle, holders ≥ 50%, ≥ 50% fresh (≤ 6 h), ≥ 2 buyers | none |
| C | A | dev linked to a known funding network (`how` ∈ funder, funder2, dev_reuse, exchange_amount, insider) |

Success on the holdout = mean net per trade > +2% at 0.25 SOL, and positive in both halves
of the holdout.

## D: Launch-Structure Engine (union, one entry per coin, first trigger wins)

| member | trigger | condition | exit |
|---|---|---|---|
| op_quality_create | end of create tx | operator has ≥ 3 resolved launches and ≥ 50% of them reached 2× after the first candle | hold 15 s, SL −15% |
| rm_g0 strict | end of creator's bundle (same slot) | create-slot holders ≥ 50% of supply, ≥ 50% of create-slot buyers fresh (≤ 6 h), ≥ 2 buyers | trailing (arm +20%, give back 10), SL −15%, max 15 s |
| rm_b0 broad | first-candle close | holders ≥ 30%, ≥ 50% fresh (≤ 2 h), ≥ 2 buyers | same trailing |
| b0_bigbundle_lowcrowd | first-candle close | create-slot SOL ≥ 5, ≤ 1 non-fresh create-slot buyer | same trailing |
| b0_fresh_bundle | first-candle close | ≥ 60% of ≥ 3 create-slot buyers fresh (≤ 2 h) | same trailing |

Entry price limit: +100% over the decision price. Discovery (data/pilot, 25.6 h): 793 trades,
+3.9%/trade (halves +3.9% / +3.7%), 19/26 hours positive, 310 operators, +7.7 SOL at 0.25 SOL.

## E: D + anti-farm (frozen before the holdout)

Same members and exits as D, plus:
* skip coins whose operator already has > 20 launches (FOMO's "farming": serial operators
  learn to dump on snipers; on discovery these had a 23% catastrophic-loss rate and ~0 mean);
* bundle-based members (run-mode, big bundle, fresh bundle) need ≥ 12 create-slot buyers
  (professional bundles; smaller ones carried no edge).

Discovery: 416 trades, +7.5%/trade (median +8.6%), 58% wins, halves +7.3% / +9.5%,
17/23 hours positive, 55 operators, +7.76 SOL at 0.25 SOL.

## Holdout result for A–E (data/pilot2, 15.8 h, Oct 30 20:45 → Oct 31 12:33 UTC)

E: −0.2%/trade (86 trades); D: −4.1%/trade (305). The discovery edge did not replicate,
not even in the same UTC hours (18–24 UTC: E +7.5% discovery vs −0.2% holdout). Rejected.

## F: pre-registered before the forward test (forward = Nov 1 14:10 → 18:10 UTC)

Selected because it is positive on BOTH days under realistic live conditions
(entry always ≥ 1 slot late, 3.5M lamports priority per attempt on both legs + tips,
sells 3–8 slots late), and its whole neighbourhood in the grid agrees:

* signal `rm_g0_s0.5_f0.7_w7200_n2`: at the end of the creator's bundle, create-slot holders
  ≥ 50% of supply, ≥ 70% of create-slot buyers freshly funded (≤ 2 h), ≥ 2 buyers
* exit `q5`: hold 5 s (SL −25%)
* discovery +1.3%/trade (141), holdout +1.6%/trade (38) at 0.25 SOL, realistic settings

Forward success = mean net > 0 under the same realistic settings, and the family average
(the 18 pairs positive on both days) > 0.

## G (frozen 2026-11-01 23:58 UTC, before the gap period 464188821–464259868 and live data)

Video "run mode" entry + quick exit, tuned on the quick-exit grid across holdout / discovery / forward.

* Entry: first-candle close (`rm_b0`): create-slot buyers ≥ 2 (n2) holding ≥ 30% of supply (s0.3),
  ≥ 30% of them fresh-funded within 1800 s before launch (f0.3, w1800). Same anti-farm and bundle
  size rules as the signal lab.
* Exit `k_h10_sl0.25_notp_off`: hold 10 s, stop −25%, no take-profit, no dump reaction.
* Realism: `--live-real` (measured live costs 3.5M lamports priority + 0.001 SOL tip per leg, 3–8 slot
  late sells), stake 0.25 SOL.
* Family for robustness: s0.3 × f{0.3, 0.5} × w1800 × n{2, 3} × hold {8, 10} × stop {0.15, 0.25}.

Results at freeze time (mean per trade / without best 2 / trades):
holdout +5.9% / +3.2% / 79, discovery +10.1% / +8.8% / 204, forward +5.4% / −4.8% / 11.

Pass rule for the gap period and each new day: trade-weighted mean > 0 and without-best-2 > 0
for the frozen pair, and ≥ 60% of the family positive.

## G2 (frozen 2026-11-02 ~00:35 UTC, before the gap period and live data)

G plus two rules from the videos, both consistent across holdout / discovery / forward:
* known operator only: launch attributed to an operator with ≥ 1 earlier launch (`how` ≠ 0);
  unknown operators lost in every period (−1.4% / −3.2% / −5.2%);
* first-candle cap: entry market cap ≤ 160 SOL (≈ $15K; FOMO's rule 2).
Exit unchanged: hold 10 s, stop −25%, no TP, no dump reaction.

Scoring (all costs, refused buys charged at their fee): racing fees 0.009 SOL / round trip and normal
fees 0.0026 SOL / round trip. At freeze, racing: holdout +3.6% (58), discovery +10.1% (151),
forward +15.2% (5); normal: +6.2% / +12.7% / +17.8%.

Pass rule for the gap period and each new day: mean > 0 at racing fees, without-best-2 > 0 at
normal fees, ≥ 10 trades per day.

## Archive test of G and G2 (pre-registered 2026-11-02 01:10 UTC, before any archived day was replayed)

No parameter of G or G2 changes. Configs: `config/eval/G.toml`, `config/eval/G2.toml` (as committed in
cfe8fcf / 51d1d42).

* **Data**: the Solana ledger archive (Old Faithful, `files.old-faithful.net`), epochs 1058–1071 and
  epoch 1072 up to slot 463,424,017 (≈ Oct 16 → Oct 30 20:45 UTC). None of it was ever looked at.
  Imported with `carstream` (same `TxView` → `decode_tx` path as the RPC backfill). Parity checked on
  1,500 slots of epoch 1073 against the RPC-recorded pilot2 data: blocks, creates, fundings and
  token transfers identical; trades identical except the quote fields of non-SOL-quoted coins
  (zeroed by the older pilot2 decoder; G/G2 never trade those). `--slim` drops the signatures of
  funding / transfer events, which the brain never reads.
* **Replay**: one continuous `lab replay --lse --live-costs --late-sells` from the first archived
  slot through the end of the existing data, operator book carried across days.
  The first imported epoch (1058) is warm-up and not scored.
* **Day** = UTC calendar day of the decision slot. Only complete days inside the archive are
  scored (Oct 17 … Oct 29 if all epochs import; whatever completes otherwise).
* **Costs**: racing fees (measured live: 3.5M lamports priority + 0.001 SOL tip per leg, refused buys
  charged) and normal fees (0.0026 SOL per round trip), stake 0.25 SOL.
* **Pass (go to canary)**, G2: ≥ 14 scored days in total (archive days + the already recorded
  out-of-sample days count; in-sample holdout/discovery do not), ≥ 70% of days profitable at racing
  fees, overall mean > 0 at racing fees, and overall mean without the best 2 trades > 0.
  Also reported: per-day mean, trades/day, worst day, longest losing streak, max drawdown in SOL.
* **Fail**: anything else. G2 is then rejected as a strategy in its current form, and the
  next step is not re-tuning on these days (that would turn them in-sample).

### Research protocol for anything after G2 (fixed 2026-11-02 ~02:40 UTC, before any archive day beyond Oct 30 14:30–20:45 UTC was replayed)

* **Development** = archive epochs 1058–1064 (≈ Oct 15 → Oct 23). New ideas are designed and tuned
  only here (and on the old in-sample holdout/discovery days).
* **Validation** = archive epochs 1065–1072 (≈ Oct 23 → Oct 30 20:45). Touched once per frozen candidate.
* **Final test** = everything recorded after Oct 30 20:45 that the candidate was not tuned on
  (forward, gap, live) plus every new day the recorder adds.
* Only G / G2 aggregates (per day) are looked at on validation days; their trade-level details are
  studied on development days only.

### Archive verdict (2026-11-02 06:45 UTC)

G2 on the 6 scored unseen days Oct 24–29 (warm-up Oct 23): 849 trades, −8.0% per trade at racing fees
(−5.4% normal), 1 of 6 days profitable, −16.9 SOL at 0.25 SOL, max drawdown 19.0 SOL. **Fail.**
G: 1,416 trades, −8.5%, 0 of 6 days. **Fail.** Both rejected; not re-tuned on these days.

## G3a / G3b (frozen 2026-11-02 14:25 UTC, rules imported from the copy3 HoodSniper live system)

Nothing below was tuned on Solana data; the three additions are copy3's live risk/exit rules:
* **G3a** = G2 + team cool-off (after a losing trade, skip the same operator for 5 min) + loss-streak
  pause (4 losses in a row → no new entries for 10 min). `config/eval/G3a.toml`.
* **G3b** = G3a + exits: trailing stop arms at +20% and sells on a 15-point giveback, stop −20%,
  timeout 60 s, exit on hard dump signals (big sell, insider consolidation / transfer-out, operator
  wallet, transfer-fed seller). `config/eval/G3b.toml`.

**Test data**: archive development days, epochs 1058–1064 (Oct 15 → Oct 22), on which neither G2 nor
G3 has ever been replayed. Continuous replay (`lab replay --lse --live-costs --late-sells`), first
epoch = warm-up, per-UTC-day scoring with `research/archive/score_days.py`. G2 is replayed on the
same days as the reference. An interim run with 1059 as warm-up may be reported while 1058 imports;
the verdict uses the full run.

**Pass** (each candidate on its own): ≥ 70% of scored days profitable at racing fees, overall mean
> 0 at racing fees, overall mean without the best 2 trades > 0. Then, and only then, the same
candidate goes to the validation days already used for G2 (Oct 24–29, seen only as G2 aggregates)
as a second check, and to paper trading on new live days.

## G3c / G2i (frozen 2026-11-02 15:20 UTC): copy3's idle-team rule

copy3's strongest live rule: buy only when at least one tax-exempt team wallet did NOT buy in the
launch second (all team wallets buying at once = the bundle that dumps within seconds).
pump.fun analog (`structure::idle_team`): wallets freshly funded within 30 min before launch by the
non-hub funders of the create-slot buyers or of the dev, that did not buy in the create slot; ≥ 1 required.
* **G3c** = G3b + idle-team ≥ 1 (`config/eval/G3c.toml`), the faithful copy3 analog.
* **G2i** = G2 + idle-team ≥ 1 (`config/eval/G2i.toml`), isolates the filter.
Same test data, replay, scoring and pass rule as G3a / G3b (development days 1058–1064).

### Verdict on the copy3-derived candidates (2026-11-02, development days Oct 16–21)

Continuous replay, live costs, per UTC day. None pass (0/4 profitable days each):

| candidate | what it adds to G2 | trades/day | mean (racing) | win | PnL @0.25 |
|---|---|---|---|---|---|
| G2 | — | 69 | −12.2% | 31% | −8.4 SOL |
| G3a | operator cool-off + loss-streak pause | 40 | −14.9% | 28% | −6.0 SOL |
| G3b | G3a + trailing/dump exits | 43 | −14.9% | 27% | −6.4 SOL |
| G3c | G3b + idle-team ≥ 1 | 16 | −13.6% | 33% | −2.2 SOL |
| **G2i** | **idle-team ≥ 1 only** | 36 | **−5.0%** | **42%** | −1.8 SOL |

The idle-team rule is the one that carries real signal (G2i: win rate 31%→42%, loss per trade more than halved), confirming copy3's "all team wallets buying at once = the bundle that dumps" observation transfers to pump.fun. But it is not enough: still negative every day. The pause/cool-off/exit rules only cut volume. Rejected as strategies; the idle-team feature is kept.

## G4 (frozen 2026-11-02 16:00 UTC): G2i + prolific-operator filter

Developed on dev days Oct 18–21 only. Within the G2i-qualifying launches, the one entry feature that
separated the +16% time-stop winners from the −54% stop-loss losers was the operator's track record:
winners' operators had a median 46 prior launches, losers 12. G4 = G2i + `b0_min_op_prior = 20`
(operator must have ≥ 20 earlier launches). `config/eval/G4.toml`.

Dev-day result: +1.9%/trade, 58% win, 74 trades — the first positive expectancy in the family.
Caveat recorded before the validation test: the mean leans on Oct 21 (9 trades, +27%); Oct 19 was
−4.5%, Oct 20 ~flat. The win-rate lift (42%→58%) is broad; the mean may be tail-driven.

**Validation (Oct 24–29, never touched): FAIL.** −10.7%/trade, 48% win, 2/6 days positive, −2.35 SOL
over 88 trades. Worse than G2i. The dev-day edge was Oct-21 overfitting, exactly as the caveat warned.
The prolific-operator filter is rejected.

### What validation DID confirm: the idle-team filter generalizes (but isn't enough)

On the same never-touched days, G2i beats G2 the same way it did on dev: win rate 45%→49%, loss per
trade −8.0%→−7.0%, and it keeps the good days while cutting the bad ones (Oct 29 +9.8% vs G2 +2.0%;
Oct 26 −5.0 SOL vs −6.8). So copy3's "a team wallet sat out" signal is real and transfers. But across
~20 unseen days (dev + validation + the earlier holdout/forward/gap/live) every launch-snipe variant
is negative overall: the edge shows up only on scattered good days (Oct 29, Oct 30 eve–Nov 1) that
cannot be identified in advance. idle-team is kept as a feature; no variant is tradeable.
