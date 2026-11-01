# Replay summary

- events: 5241434 replayed in 10.5s; chain time covered: 6.94 h
- launches: 8067 (tradable SOL-paired: 3671), resolved rows: 3572
- attributed to an operator seen before: 2317 (63.1%), late via insiders: 20
- attribution paths: [("dev_reuse", 1702), ("new", 1354), ("funder", 501), ("exchange_amount", 69), ("funder2", 45)]
- operators with ≥2 / ≥5 / ≥10 resolved launches: 394 / 139 / 57
- population prior (EMA of honest shadow returns): -0.034
- gate: pass 1553 | blocked by risk 1409 | reasons [("few_launches", 2575), ("pass_flow", 1553), ("low_lcb", 1096)]

## Shadow results by exit variant (all resolved launches, stake-normalized, after fees+tips)

| variant | n | mean ret | median | win rate |
|---|---|---|---|---|
| t2s | 3572 | -0.039 | -0.050 | 11.3% |
| scalp40 | 3572 | -0.038 | -0.050 | 12.9% |
| tp70 | 3572 | -0.040 | -0.050 | 13.3% |
| tp100 | 3572 | -0.042 | -0.050 | 13.2% |
| tp150 | 3572 | -0.043 | -0.050 | 13.1% |
| tp100_raw | 3572 | -0.054 | -0.050 | 11.1% |
| tp200_raw | 3572 | -0.059 | -0.050 | 9.7% |
| trail60 | 3572 | -0.042 | -0.050 | 13.5% |
| ladder | 3572 | -0.042 | -0.050 | 13.4% |
| clock30 | 3572 | -0.041 | -0.050 | 12.6% |
| tp70_all | 3572 | -0.039 | -0.050 | 13.0% |
| t60s | 3572 | -0.053 | -0.050 | 10.6% |
| b1_t2s | 3572 | -0.048 | -0.050 | 8.0% |
| b1_scalp40 | 3572 | -0.049 | -0.050 | 9.4% |
| b1_tp70 | 3572 | -0.050 | -0.050 | 10.1% |
| b1_tp100 | 3572 | -0.051 | -0.050 | 10.5% |
| b1_trail60 | 3572 | -0.052 | -0.050 | 10.3% |
| b1_t60s | 3572 | -0.067 | -0.050 | 9.3% |
| ds_tp50 | 1884 | -0.060 | -0.050 | 9.7% |
| ds_tp100 | 1884 | -0.086 | -0.052 | 11.0% |
| dp_tp40 | 1630 | -0.073 | -0.083 | 19.3% |
| dp_tp100 | 1630 | -0.087 | -0.112 | 16.7% |
| flow_h60 | 0 | NaN | NaN | NaN% |

### Same, launches by operators with ≥2 prior launches (1872 rows)

| variant | n | mean ret | median | win rate |
|---|---|---|---|---|
| t2s | 1872 | -0.044 | -0.050 | 12.0% |
| scalp40 | 1872 | -0.041 | -0.050 | 13.7% |
| tp70 | 1872 | -0.045 | -0.050 | 13.1% |
| tp100 | 1872 | -0.045 | -0.050 | 13.1% |
| tp150 | 1872 | -0.048 | -0.050 | 12.7% |
| tp100_raw | 1872 | -0.066 | -0.050 | 9.5% |
| tp200_raw | 1872 | -0.074 | -0.050 | 7.7% |
| trail60 | 1872 | -0.046 | -0.050 | 13.2% |
| ladder | 1872 | -0.046 | -0.050 | 13.2% |
| clock30 | 1872 | -0.043 | -0.050 | 11.7% |
| tp70_all | 1872 | -0.043 | -0.050 | 12.5% |
| t60s | 1872 | -0.064 | -0.050 | 9.2% |
| b1_t2s | 1872 | -0.052 | -0.050 | 8.7% |
| b1_scalp40 | 1872 | -0.052 | -0.050 | 10.0% |
| b1_tp70 | 1872 | -0.054 | -0.050 | 10.3% |
| b1_tp100 | 1872 | -0.054 | -0.050 | 10.6% |
| b1_trail60 | 1872 | -0.056 | -0.050 | 10.5% |
| b1_t60s | 1872 | -0.078 | -0.050 | 8.1% |
| ds_tp50 | 1116 | -0.060 | -0.050 | 9.5% |
| ds_tp100 | 1116 | -0.096 | -0.059 | 10.3% |
| dp_tp40 | 894 | -0.091 | -0.132 | 18.2% |
| dp_tp100 | 894 | -0.112 | -0.165 | 14.7% |
| flow_h60 | 0 | NaN | NaN | NaN% |

Selection-honest shadow (variant chosen before each launch): n=3511 mean -0.038 win 13.3%

## Price path after a realistic entry (median of max gain within window)

| set | n | 2s | 5s | 10s | 30s | 60s | 3m | 10m |
|---|---|---|---|---|---|---|---|---|
| all | 3445 | 0.00 | 0.00 | 0.00 | 0.00 | 0.00 | 0.00 | 0.00 |
| repeat operators | 1840 | 0.00 | 0.00 | 0.00 | 0.00 | 0.00 | 0.00 | 0.00 |

## Real (gated) trades

- entries attempted 144 | refused by price limit 0 | filled 144
- win rate 68.1% | mean ret 0.163 | median 0.151 | total PnL 2.3483 SOL
- max drawdown 0.1660 SOL

| exit reason | n | PnL SOL |
|---|---|---|
| time_stop | 68 | 1.2632 |
| trailing | 29 | 1.0242 |
| stop_loss | 20 | -0.6858 |
| idle | 18 | -0.1200 |
| take_profit | 8 | 0.8341 |
| migrated | 1 | 0.0326 |

| UTC day | trades | sum of returns |
|---|---|---|
| 20758 | 144 | 23.483 |

## Go-live checklist (thresholds from docs/RUNBOOK.md)

| check | value | threshold | |
|---|---|---|---|
| real trades | 144 | ≥ 60 | PASS |
| mean return / trade | 0.1631 | > 0.04 | PASS |
| bootstrap 95% lower bound | 0.1049 | > 0 | PASS |
| profitable days | 1/1 | ≥ 70% | PASS |
| best day share of PnL | 1.00 | < 0.35 | FAIL |
| best operator share of PnL | 1.00 | < 0.30 | FAIL |
| max drawdown / bankroll | 0.033 | < 0.25 | PASS |

Stress levels are separate runs (`--stress 1`, `--stress 2`).

## Operators with most resolved launches

| op | launches | devs | honest n | honest mean | win | gate |
|---|---|---|---|---|---|---|
| 1923 | 176 | 1 | 176 | -0.049 | 1% | false (low_lcb) |
| 715 | 70 | 1 | 70 | 0.001 | 36% | false (low_lcb) |
| 195 | 70 | 9 | 70 | -0.001 | 47% | false (low_lcb) |
| 468 | 69 | 23 | 69 | -0.050 | 0% | false (low_lcb) |
| 907 | 57 | 1 | 57 | -0.050 | 0% | false (low_lcb) |
| 766 | 52 | 1 | 27 | -0.050 | 0% | false (low_lcb) |
| 1504 | 48 | 1 | 48 | -0.050 | 0% | false (low_lcb) |
| 254 | 48 | 2 | 48 | 0.002 | 50% | false (low_lcb) |
| 1473 | 47 | 1 | 47 | -0.050 | 0% | false (low_lcb) |
| 321 | 40 | 22 | 40 | -0.011 | 20% | false (low_lcb) |
| 573 | 36 | 36 | 36 | -0.050 | 0% | false (low_lcb) |
| 149 | 36 | 1 | 36 | -0.050 | 0% | false (low_lcb) |
| 111 | 34 | 34 | 34 | 0.018 | 18% | false (low_lcb) |
| 33 | 33 | 1 | 33 | -0.104 | 15% | false (low_lcb) |
| 719 | 32 | 1 | 32 | 0.023 | 38% | false (low_lcb) |
| 2 | 28 | 3 | 28 | -0.013 | 25% | false (low_lcb) |
| 16 | 28 | 1 | 28 | -0.045 | 7% | false (low_lcb) |
| 15 | 25 | 2 | 25 | -0.034 | 28% | false (low_lcb) |
| 94 | 25 | 1 | 25 | -0.006 | 64% | false (low_lcb) |
| 1169 | 24 | 1 | 23 | -0.010 | 17% | false (low_lcb) |
| 855 | 24 | 1 | 24 | -0.060 | 0% | false (low_lcb) |
| 677 | 22 | 9 | 22 | -0.052 | 5% | false (low_lcb) |
| 1051 | 22 | 1 | 22 | -0.109 | 14% | false (low_lcb) |
| 277 | 21 | 1 | 21 | -0.048 | 19% | false (low_lcb) |
| 1110 | 20 | 5 | 20 | -0.050 | 0% | false (low_lcb) |
