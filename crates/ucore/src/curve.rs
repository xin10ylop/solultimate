//! pump.fun constant-product bonding curve math (integer, mirrors the program/SDK).

use crate::pump::TOKEN_SUPPLY;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Curve {
    pub vsol: u64,
    pub vtok: u64,
    pub rsol: u64,
    pub rtok: u64,
}

fn ceil_div(a: u128, b: u128) -> u128 {
    if b == 0 {
        0
    } else {
        a.div_ceil(b)
    }
}

/// Fee on an amount, rounded up (per fee component).
pub fn fee_on(amount: u64, bps: u32) -> u64 {
    ceil_div(amount as u128 * bps as u128, 10_000) as u64
}

impl Curve {
    pub fn initial() -> Curve {
        Curve {
            vsol: crate::pump::INITIAL_VIRTUAL_SOL,
            vtok: crate::pump::INITIAL_VIRTUAL_TOKEN,
            rsol: 0,
            rtok: crate::pump::INITIAL_REAL_TOKEN,
        }
    }
    /// Lamports per base unit (6-decimals token).
    pub fn price(&self) -> f64 {
        if self.vtok == 0 {
            return f64::INFINITY;
        }
        self.vsol as f64 / self.vtok as f64
    }
    /// Market cap in SOL.
    pub fn mcap_sol(&self) -> f64 {
        self.price() * TOKEN_SUPPLY as f64 / 1e9
    }
    pub fn k(&self) -> u128 {
        self.vsol as u128 * self.vtok as u128
    }
    /// Tokens out for `sol_net` lamports entering the curve (after fees).
    pub fn tokens_for_sol(&self, sol_net: u64) -> u64 {
        let out = (sol_net as u128 * self.vtok as u128) / (self.vsol as u128 + sol_net as u128);
        (out as u64).min(self.rtok)
    }
    /// SOL (net, before fees) required to buy exactly `tokens`.
    pub fn sol_for_tokens(&self, tokens: u64) -> u64 {
        let t = tokens.min(self.rtok) as u128;
        if t as u64 >= self.vtok {
            return u64::MAX;
        }
        ((t * self.vsol as u128) / (self.vtok as u128 - t) + 1) as u64
    }
    /// Buy spending exactly `spend` lamports including fees (`buy_exact_quote_in`).
    /// Returns (tokens_out, sol_net_into_curve, fee).
    pub fn buy_quote_in(&self, spend: u64, fee_bps: u32) -> (u64, u64, u64) {
        let input = (spend as u128 * 10_000 / (10_000 + fee_bps as u128)) as u64;
        let tokens = self.tokens_for_sol(input);
        if tokens == 0 {
            return (0, 0, 0);
        }
        let sol_net = self.sol_for_tokens(tokens).min(input);
        let fee = fee_on(sol_net, fee_bps);
        (tokens, sol_net, fee)
    }
    /// Sell `tokens`. Returns (sol_to_user, sol_gross_out_of_curve, fee).
    pub fn sell(&self, tokens: u64, fee_bps: u32) -> (u64, u64, u64) {
        let gross = ((tokens as u128 * self.vsol as u128) / (self.vtok as u128 + tokens as u128)) as u64;
        let fee = fee_on(gross, fee_bps);
        (gross.saturating_sub(fee), gross, fee)
    }
    pub fn apply_buy(&mut self, sol_net: u64, tokens: u64) {
        self.vsol += sol_net;
        self.rsol += sol_net;
        self.vtok -= tokens.min(self.vtok);
        self.rtok -= tokens.min(self.rtok);
    }
    pub fn apply_sell(&mut self, tokens: u64, sol_gross: u64) {
        self.vtok += tokens;
        self.rtok += tokens;
        self.vsol -= sol_gross.min(self.vsol);
        self.rsol -= sol_gross.min(self.rsol);
    }
    /// The state that would exist if `extra_sol` more (net) lamports had entered the curve,
    /// keeping k constant. Used to account for our own position in counterfactual replay.
    pub fn shifted(&self, extra_sol: i64) -> Curve {
        if extra_sol == 0 {
            return *self;
        }
        let k = self.k();
        let vsol = (self.vsol as i128 + extra_sol as i128).max(1) as u128;
        let vtok = (k / vsol) as u64;
        let dt = self.vtok as i128 - vtok as i128;
        Curve {
            vsol: vsol as u64,
            vtok,
            rsol: (self.rsol as i128 + extra_sol as i128).max(0) as u64,
            rtok: (self.rtok as i128 - dt).max(0) as u64,
        }
    }
    /// Progress toward completion in [0,1] (real tokens sold / initial real tokens).
    pub fn progress(&self) -> f64 {
        1.0 - self.rtok as f64 / crate::pump::INITIAL_REAL_TOKEN as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn observed_mainnet_trade_matches() {
        // Real consecutive trades on GyXw...pump (slot ~464106000):
        // previous post-state: vsol=49051573042 vtok=656248068213754
        let pre = Curve { vsol: 49_051_573_042, vtok: 656_248_068_213_754, rsol: 19_051_573_042, rtok: 376_348_068_213_754 };
        // 'buy' of 3_719_413_332_821 tokens cost 279_593_967 net lamports
        assert_eq!(pre.sol_for_tokens(3_719_413_332_821), 279_593_967);
        // fees: 95 bps protocol (2_656_143) + 30 bps creator (838_782), each rounded up
        assert_eq!(fee_on(279_593_967, 95), 2_656_143);
        assert_eq!(fee_on(279_593_967, 30), 838_782);
        let mut post = pre;
        post.apply_buy(279_593_967, 3_719_413_332_821);
        assert_eq!(post.vsol, 49_331_167_009);
        assert_eq!(post.vtok, 652_528_654_880_933);
    }
    #[test]
    fn shift_keeps_k() {
        let c = Curve::initial();
        let s = c.shifted(1_000_000_000);
        let (tok, _, _) = c.buy_quote_in(1_000_000_000, 0);
        assert!((s.vtok as i128 - (c.vtok - tok) as i128).abs() < 1_000_000);
    }
}
