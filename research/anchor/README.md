# Anchored-token arbitrage (in progress)

Pump coins are increasingly quoted in tokens with an external price: wrapped assets (wXMR, WBTC,
DOGE, HYPE), tokenized stocks (xStocks, Backpack Securities) and pre-IPO tokens (PreStocks).
Their routed flow passes through the quote token's thin Solana pools and pushes it off its
anchor, which trades deep on Hyperliquid (crypto perps and the `xyz` stock / pre-IPO perps).

| script | what it measures |
|---|---|
| `fetch.py`, `dev.py` | minute candles of Solana pools (GeckoTerminal) vs Hyperliquid 1m candles: gap size and persistence |
| `poll.py` | live executable gaps every ~2 min: real Jupiter quotes ($500 / $3,000) vs Hyperliquid best bid/ask, all fees included |
| `evalpoll2.py` | hedged-inventory capture per token and size, USD/day and % of capital/day |
| `roundtrip.py` | SOL → X → SOL Jupiter round trips for memecoins quoted in anchored tokens (atomic arbitrage check) |
