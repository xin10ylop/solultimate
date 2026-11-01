# Replay summary

- events: 4088891 replayed in 6.7s; chain time covered: 5.65 h
- launches: 5875 (tradable SOL-paired: 2648), resolved rows: 2488
- attributed to an operator seen before: 1602 (60.5%), late via insiders: 15
- attribution paths: [("dev_reuse", 1107), ("new", 1046), ("funder", 412), ("exchange_amount", 49), ("funder2", 34)]
- operators with ≥2 / ≥5 / ≥10 resolved launches: 310 / 108 / 49
- population prior (EMA of honest shadow returns): -0.041
- gate: pass 0 | blocked by risk 0 | reasons [("few_launches", 1976), ("low_lcb", 672)]

## Shadow results by exit variant (all resolved launches, stake-normalized, after fees+tips)

| variant | n | mean ret | median | win rate |
|---|---|---|---|---|
| t2s | 2488 | -0.036 | -0.050 | 13.1% |
| scalp40 | 2488 | -0.037 | -0.050 | 14.2% |
| tp70 | 2488 | -0.040 | -0.050 | 14.2% |
| tp100 | 2488 | -0.041 | -0.050 | 14.3% |
| tp150 | 2488 | -0.042 | -0.050 | 14.1% |
| tp100_raw | 2488 | -0.057 | -0.050 | 12.1% |
| tp200_raw | 2488 | -0.063 | -0.050 | 10.7% |
| trail60 | 2488 | -0.041 | -0.050 | 14.5% |
| ladder | 2488 | -0.041 | -0.050 | 14.4% |
| clock30 | 2488 | -0.040 | -0.050 | 13.3% |
| tp70_all | 2488 | -0.038 | -0.050 | 14.1% |
| t60s | 2488 | -0.060 | -0.050 | 11.7% |
| b1_t2s | 2488 | -0.048 | -0.050 | 9.1% |
| b1_scalp40 | 2488 | -0.049 | -0.050 | 9.9% |
| b1_tp70 | 2488 | -0.052 | -0.050 | 10.4% |
| b1_tp100 | 2488 | -0.051 | -0.050 | 11.1% |
| b1_trail60 | 2488 | -0.053 | -0.050 | 10.8% |
| b1_t60s | 2488 | -0.072 | -0.050 | 10.1% |
| ds_tp50 | 1343 | -0.058 | -0.052 | 10.3% |
| ds_tp100 | 1343 | -0.085 | -0.055 | 11.8% |
| dp_tp40 | 1209 | -0.070 | -0.081 | 20.3% |
| dp_tp100 | 1209 | -0.085 | -0.111 | 17.7% |

### Same, launches by operators with ≥2 prior launches (1186 rows)

| variant | n | mean ret | median | win rate |
|---|---|---|---|---|
| t2s | 1186 | -0.040 | -0.050 | 15.3% |
| scalp40 | 1186 | -0.039 | -0.050 | 16.9% |
| tp70 | 1186 | -0.042 | -0.050 | 15.5% |
| tp100 | 1186 | -0.042 | -0.050 | 15.9% |
| tp150 | 1186 | -0.046 | -0.050 | 15.3% |
| tp100_raw | 1186 | -0.074 | -0.050 | 10.7% |
| tp200_raw | 1186 | -0.088 | -0.050 | 8.9% |
| trail60 | 1186 | -0.044 | -0.050 | 15.8% |
| ladder | 1186 | -0.045 | -0.050 | 15.7% |
| clock30 | 1186 | -0.043 | -0.050 | 13.6% |
| tp70_all | 1186 | -0.041 | -0.050 | 15.2% |
| t60s | 1186 | -0.079 | -0.050 | 10.7% |
| b1_t2s | 1186 | -0.050 | -0.050 | 11.1% |
| b1_scalp40 | 1186 | -0.051 | -0.050 | 12.2% |
| b1_tp70 | 1186 | -0.053 | -0.050 | 12.0% |
| b1_tp100 | 1186 | -0.052 | -0.050 | 12.6% |
| b1_trail60 | 1186 | -0.055 | -0.050 | 12.4% |
| b1_t60s | 1186 | -0.091 | -0.050 | 9.2% |
| ds_tp50 | 769 | -0.058 | -0.053 | 10.4% |
| ds_tp100 | 769 | -0.098 | -0.072 | 11.4% |
| dp_tp40 | 646 | -0.092 | -0.132 | 19.2% |
| dp_tp100 | 646 | -0.117 | -0.175 | 15.0% |

Selection-honest shadow (variant chosen before each launch): n=2469 mean -0.038 win 14.5%

## Price path after a realistic entry (median of max gain within window)

| set | n | 2s | 5s | 10s | 30s | 60s | 3m | 10m |
|---|---|---|---|---|---|---|---|---|
| all | 2398 | 0.00 | 0.00 | 0.00 | 0.00 | 0.01 | 0.01 | 0.01 |
| repeat operators | 1170 | 0.00 | 0.00 | 0.00 | 0.00 | 0.00 | 0.01 | 0.01 |

## Real (gated) trades

- entries attempted 0 | refused by price limit 0 | filled 0
- win rate NaN% | mean ret NaN | median NaN | total PnL -0.0000 SOL
- max drawdown 0.0000 SOL

| exit reason | n | PnL SOL |
|---|---|---|

| UTC day | trades | sum of returns |
|---|---|---|

## Go-live checklist (thresholds from docs/RUNBOOK.md)

| check | value | threshold | |
|---|---|---|---|
| real trades | 0 | ≥ 60 | FAIL |
| mean return / trade | NaN | > 0.04 | FAIL |
| bootstrap 95% lower bound | NaN | > 0 | FAIL |
| profitable days | 0/0 | ≥ 70% | FAIL |
| best day share of PnL | NaN | < 0.35 | FAIL |
| best operator share of PnL | NaN | < 0.30 | FAIL |
| max drawdown / bankroll | 0.000 | < 0.25 | PASS |

Stress levels are separate runs (`--stress 1`, `--stress 2`).

## Operators with most resolved launches

| op | launches | devs | honest n | honest mean | win | gate |
|---|---|---|---|---|---|---|
| 468 | 69 | 23 | 69 | -0.050 | 0% | false (low_lcb) |
| 195 | 67 | 7 | 67 | 0.002 | 49% | false (low_lcb) |
| 907 | 47 | 1 | 47 | -0.050 | 0% | false (low_lcb) |
| 715 | 44 | 1 | 44 | 0.019 | 45% | false (low_lcb) |
| 254 | 43 | 2 | 43 | -0.001 | 49% | false (low_lcb) |
| 766 | 34 | 1 | 19 | -0.050 | 0% | false (low_lcb) |
| 1473 | 33 | 1 | 33 | -0.050 | 0% | false (low_lcb) |
| 321 | 33 | 18 | 33 | -0.020 | 18% | false (low_lcb) |
| 33 | 30 | 1 | 30 | -0.104 | 13% | false (low_lcb) |
| 2 | 28 | 3 | 28 | -0.013 | 25% | false (low_lcb) |
| 573 | 27 | 27 | 27 | -0.050 | 0% | false (low_lcb) |
| 111 | 26 | 26 | 26 | 0.037 | 23% | false (low_lcb) |
| 94 | 25 | 1 | 25 | -0.006 | 64% | false (low_lcb) |
| 15 | 24 | 2 | 24 | -0.033 | 29% | false (low_lcb) |
| 855 | 23 | 1 | 23 | -0.051 | 0% | false (low_lcb) |
| 277 | 21 | 1 | 21 | -0.048 | 19% | false (low_lcb) |
| 1110 | 20 | 5 | 20 | -0.050 | 0% | false (low_lcb) |
| 16 | 20 | 1 | 20 | -0.042 | 10% | false (low_lcb) |
| 1504 | 19 | 1 | 19 | -0.050 | 0% | false (low_lcb) |
| 852 | 19 | 2 | 19 | -0.050 | 0% | false (low_lcb) |
| 837 | 18 | 18 | 18 | 0.002 | 39% | false (low_lcb) |
| 677 | 17 | 8 | 17 | -0.061 | 6% | false (low_lcb) |
| 149 | 16 | 1 | 16 | -0.050 | 0% | false (low_lcb) |
| 9 | 16 | 1 | 16 | -0.125 | 0% | false (low_lcb) |
| 572 | 15 | 1 | 15 | -0.040 | 33% | false (low_lcb) |
