//! pump.fun bonding-curve program: addresses, discriminators and event decoding.
//!
//! Layouts follow `idl/pump.json` from pump-fun/pump-public-docs (Oct 2026). Decoding is
//! prefix-tolerant: fields appended by later program upgrades are optional, and anything
//! after the last known field is ignored.

use crate::pk::{pk, Pk};

pub const PUMP_PROGRAM: Pk = pk("6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P");
pub const PUMP_AMM_PROGRAM: Pk = pk("pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA");
pub const PUMP_FEES_PROGRAM: Pk = pk("pfeeUxB6jkeY1Hxd7CsFCAjcbHA9rWtchMGdZ6VojVZ");
pub const MAYHEM_PROGRAM: Pk = pk("MAyhSmzXzV1pTf7LsNkrNwkWKTo4ougAJ1PPg47MD4e");
pub const GLOBAL: Pk = pk("4wTV1YmiEkRvAtNtsSGPtUrqRYQMe5SKy2uB4Jjaxnjf");
pub const EVENT_AUTHORITY: Pk = pk("Ce6TQqeHC9p8KetsN6JsjHK7UTZk7nasjjnr7XxXp9F1");
pub const FEE_CONFIG: Pk = pk("8Wf5TiAheLUqBrKXeYg2JtAFFMWtKdG2BSFgqUcPVwTt");
pub const SYSTEM_PROGRAM: Pk = pk("11111111111111111111111111111111");
pub const COMPUTE_BUDGET: Pk = pk("ComputeBudget111111111111111111111111111111");
pub const TOKEN_PROGRAM: Pk = pk("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
pub const TOKEN_2022: Pk = pk("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb");
pub const ATA_PROGRAM: Pk = pk("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");
pub const WSOL: Pk = pk("So11111111111111111111111111111111111111112");
pub const VOTE_PROGRAM: Pk = pk("Vote111111111111111111111111111111111111111");

/// Buyback fee recipients (any one may be passed to v3 trades).
pub const BUYBACK_FEE_RECIPIENTS: [Pk; 8] = [
    pk("5YxQFdt3Tr9zJLvkFccqXVUwhdTWJQc1fFg2YPbxvxeD"),
    pk("9M4giFFMxmFGXtc3feFzRai56WbBqehoSeRE5GK7gf7"),
    pk("GXPFM2caqTtQYC2cJ5yJRi9VDkpsYZXzYdwYpGnLmtDL"),
    pk("3BpXnfJaUTiwXnJNe7Ej1rcbzqTTQUvLShZaWazebsVR"),
    pk("5cjcW9wExnJJiqgLjq7DEG75Pm6JBgE1hNv4B2vHXUW6"),
    pk("EHAAiTxcdDwQ3U4bU6YcMsQGaekdzLS3B5SmYo46kJtL"),
    pk("5eHhjP8JaYkz83CWwvGU2uMUXefd3AazWGx4gpcuEEYD"),
    pk("A7hAgCzFw14fejgCp387JUJRMNyz4j89JKnhtKU8piqW"),
];

/// Anchor `emit_cpi!` instruction tag (sha256("anchor:event")[..8]).
pub const EVENT_IX_TAG: [u8; 8] = [0xe4, 0x45, 0xa5, 0x2e, 0x51, 0xcb, 0x9a, 0x1d];
pub const DISC_TRADE_EVENT: [u8; 8] = [189, 219, 127, 211, 78, 230, 97, 238];
pub const DISC_CREATE_EVENT: [u8; 8] = [27, 114, 169, 77, 222, 235, 99, 118];
pub const DISC_COMPLETE_EVENT: [u8; 8] = [95, 114, 97, 156, 212, 46, 152, 8];

pub const IX_CREATE: [u8; 8] = [24, 30, 200, 40, 5, 28, 7, 119];
pub const IX_CREATE_V2: [u8; 8] = [214, 144, 76, 236, 95, 139, 49, 180];
pub const IX_BUY_EXACT_QUOTE_IN_V3: [u8; 8] = [225, 247, 80, 30, 213, 179, 132, 136];
pub const IX_BUY_V3: [u8; 8] = [7, 5, 29, 196, 245, 23, 101, 80];
pub const IX_SELL_V3: [u8; 8] = [28, 146, 222, 119, 38, 196, 105, 213];

/// Observed mainnet slot duration (Nov 2026: ~211 ms). `lab check` prints the measured value;
/// update this if the network changes its slot time again.
pub const SLOT_MS: u64 = 211;
/// Seconds → slots.
pub const fn secs(s: u64) -> u64 {
    s * 1000 / SLOT_MS
}
pub const SLOTS_PER_DAY: u64 = secs(86_400);

pub const INITIAL_VIRTUAL_SOL: u64 = 30_000_000_000;
pub const INITIAL_VIRTUAL_TOKEN: u64 = 1_073_000_000_000_000;
pub const INITIAL_REAL_TOKEN: u64 = 793_100_000_000_000;
pub const TOKEN_SUPPLY: u64 = 1_000_000_000_000_000;

/// Trade instruction name → compact code (stored in events).
pub fn ix_code(name: &str) -> u8 {
    match name {
        "buy" => 1,
        "sell" => 2,
        "buy_exact_sol_in" => 3,
        "buy_exact_quote_in" => 4,
        "buy_v2" => 5,
        "sell_v2" => 6,
        "buy_exact_quote_in_v2" => 7,
        "buy_v3" => 8,
        "sell_v3" => 9,
        "buy_exact_quote_in_v3" => 10,
        "multi_hop_swap" => 11,
        _ => 0,
    }
}

// ---------------------------------------------------------------- borsh reader

pub struct Rd<'a> {
    pub d: &'a [u8],
    pub o: usize,
}
impl<'a> Rd<'a> {
    pub fn new(d: &'a [u8]) -> Self {
        Rd { d, o: 0 }
    }
    pub fn left(&self) -> usize {
        self.d.len().saturating_sub(self.o)
    }
    pub fn u8(&mut self) -> Option<u8> {
        let v = *self.d.get(self.o)?;
        self.o += 1;
        Some(v)
    }
    pub fn bool(&mut self) -> Option<bool> {
        self.u8().map(|v| v != 0)
    }
    pub fn u16(&mut self) -> Option<u16> {
        let s = self.d.get(self.o..self.o + 2)?;
        self.o += 2;
        Some(u16::from_le_bytes(s.try_into().ok()?))
    }
    pub fn u32(&mut self) -> Option<u32> {
        let s = self.d.get(self.o..self.o + 4)?;
        self.o += 4;
        Some(u32::from_le_bytes(s.try_into().ok()?))
    }
    pub fn u64(&mut self) -> Option<u64> {
        let s = self.d.get(self.o..self.o + 8)?;
        self.o += 8;
        Some(u64::from_le_bytes(s.try_into().ok()?))
    }
    pub fn i64(&mut self) -> Option<i64> {
        self.u64().map(|v| v as i64)
    }
    pub fn pk(&mut self) -> Option<Pk> {
        let p = Pk::from_slice(self.d.get(self.o..self.o + 32)?)?;
        self.o += 32;
        Some(p)
    }
    pub fn string(&mut self) -> Option<String> {
        let n = self.u32()? as usize;
        if n > 4096 {
            return None;
        }
        let s = self.d.get(self.o..self.o + n)?;
        self.o += n;
        Some(String::from_utf8_lossy(s).into_owned())
    }
    pub fn skip(&mut self, n: usize) -> Option<()> {
        if self.left() < n {
            return None;
        }
        self.o += n;
        Some(())
    }
}

// ---------------------------------------------------------------- raw events

#[derive(Clone, Debug, Default)]
pub struct RawTrade {
    pub mint: Pk,
    pub sol_amount: u64,
    pub token_amount: u64,
    pub is_buy: bool,
    pub user: Pk,
    pub timestamp: i64,
    pub virtual_sol_reserves: u64,
    pub virtual_token_reserves: u64,
    pub real_sol_reserves: u64,
    pub real_token_reserves: u64,
    pub fee_recipient: Pk,
    pub fee_bps: u64,
    pub fee: u64,
    pub creator: Pk,
    pub creator_fee_bps: u64,
    pub creator_fee: u64,
    pub ix_name: String,
    pub mayhem: bool,
    pub cashback_bps: u64,
    pub cashback: u64,
    pub quote_mint: Pk,
    pub quote_amount: u64,
    pub virtual_quote_reserves: u64,
    pub real_quote_reserves: u64,
}

pub fn decode_trade(body: &[u8]) -> Option<RawTrade> {
    let mut r = Rd::new(body);
    let mut t = RawTrade {
        mint: r.pk()?,
        sol_amount: r.u64()?,
        token_amount: r.u64()?,
        is_buy: r.bool()?,
        user: r.pk()?,
        timestamp: r.i64()?,
        virtual_sol_reserves: r.u64()?,
        virtual_token_reserves: r.u64()?,
        real_sol_reserves: r.u64()?,
        real_token_reserves: r.u64()?,
        ..Default::default()
    };
    // Everything below was appended over time; stop gracefully.
    (|| -> Option<()> {
        t.fee_recipient = r.pk()?;
        t.fee_bps = r.u64()?;
        t.fee = r.u64()?;
        t.creator = r.pk()?;
        t.creator_fee_bps = r.u64()?;
        t.creator_fee = r.u64()?;
        let _track_volume = r.bool()?;
        let _total_unclaimed = r.u64()?;
        let _total_claimed = r.u64()?;
        let _current_sol_volume = r.u64()?;
        let _last_update = r.i64()?;
        t.ix_name = r.string()?;
        t.mayhem = r.bool()?;
        t.cashback_bps = r.u64()?;
        t.cashback = r.u64()?;
        let _buyback_bps = r.u64()?;
        let _buyback_fee = r.u64()?;
        let n = r.u32()? as usize;
        r.skip(n.checked_mul(34)?)?; // Shareholder { address: Pubkey, share_bps: u16 }
        t.quote_mint = r.pk()?;
        t.quote_amount = r.u64()?;
        t.virtual_quote_reserves = r.u64()?;
        t.real_quote_reserves = r.u64()?;
        Some(())
    })();
    Some(t)
}

#[derive(Clone, Debug, Default)]
pub struct RawCreate {
    pub name: String,
    pub symbol: String,
    pub uri: String,
    pub mint: Pk,
    pub bonding_curve: Pk,
    pub user: Pk,
    pub creator: Pk,
    pub timestamp: i64,
    pub virtual_token_reserves: u64,
    pub virtual_sol_reserves: u64,
    pub real_token_reserves: u64,
    pub token_total_supply: u64,
    pub token_program: Pk,
    pub mayhem: bool,
    pub cashback: bool,
    pub quote_mint: Pk,
    pub creator_fee_bps: u64,
    pub holder_reward: bool,
    pub depth: u8,
    pub virtual_quote_reserves: u64,
}

pub fn decode_create(body: &[u8]) -> Option<RawCreate> {
    let mut r = Rd::new(body);
    let mut c = RawCreate {
        name: r.string()?,
        symbol: r.string()?,
        uri: r.string()?,
        mint: r.pk()?,
        bonding_curve: r.pk()?,
        user: r.pk()?,
        ..Default::default()
    };
    (|| -> Option<()> {
        c.creator = r.pk()?;
        c.timestamp = r.i64()?;
        c.virtual_token_reserves = r.u64()?;
        c.virtual_sol_reserves = r.u64()?;
        c.real_token_reserves = r.u64()?;
        c.token_total_supply = r.u64()?;
        c.token_program = r.pk()?;
        c.mayhem = r.bool()?;
        c.cashback = r.bool()?;
        c.quote_mint = r.pk()?;
        c.virtual_quote_reserves = r.u64()?;
        c.creator_fee_bps = r.u64()?;
        c.holder_reward = r.bool()?;
        c.depth = r.u8()?;
        Some(())
    })();
    if c.creator == Pk::ZERO {
        c.creator = c.user;
    }
    if c.token_program == Pk::ZERO {
        c.token_program = TOKEN_PROGRAM;
    }
    Some(c)
}

#[derive(Clone, Debug, Default)]
pub struct RawComplete {
    pub user: Pk,
    pub mint: Pk,
    pub bonding_curve: Pk,
}

pub fn decode_complete(body: &[u8]) -> Option<RawComplete> {
    let mut r = Rd::new(body);
    Some(RawComplete { user: r.pk()?, mint: r.pk()?, bonding_curve: r.pk()? })
}

pub enum RawEvent {
    Trade(RawTrade),
    Create(RawCreate),
    Complete(RawComplete),
}

/// Decode an anchor event payload that starts with the 8-byte event discriminator.
pub fn decode_event(payload: &[u8]) -> Option<RawEvent> {
    if payload.len() < 8 {
        return None;
    }
    let (disc, body) = payload.split_at(8);
    if disc == DISC_TRADE_EVENT {
        decode_trade(body).map(RawEvent::Trade)
    } else if disc == DISC_CREATE_EVENT {
        decode_create(body).map(RawEvent::Create)
    } else if disc == DISC_COMPLETE_EVENT {
        decode_complete(body).map(RawEvent::Complete)
    } else {
        None
    }
}

/// True when the quote mint means "native SOL" (default key or WSOL).
pub fn is_sol_quote(q: &Pk) -> bool {
    *q == Pk::ZERO || *q == WSOL || *q == SYSTEM_PROGRAM
}
