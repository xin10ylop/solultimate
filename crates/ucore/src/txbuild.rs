//! Minimal, dependency-light Solana transaction building (legacy message format) and the
//! pump.fun v3 trade instructions. Validated against mainnet with `simulateTransaction`
//! (see `unode simcheck`).

use crate::pk::{Pk, Sig};
use crate::pump::*;
use ed25519_dalek::{Signer, SigningKey};

#[derive(Clone, Debug)]
pub struct Meta {
    pub pk: Pk,
    pub signer: bool,
    pub writable: bool,
}
impl Meta {
    pub fn w(pk: Pk) -> Meta {
        Meta { pk, signer: false, writable: true }
    }
    pub fn r(pk: Pk) -> Meta {
        Meta { pk, signer: false, writable: false }
    }
    pub fn ws(pk: Pk) -> Meta {
        Meta { pk, signer: true, writable: true }
    }
}

#[derive(Clone, Debug)]
pub struct Instruction {
    pub program: Pk,
    pub accounts: Vec<Meta>,
    pub data: Vec<u8>,
}

fn compact_u16(out: &mut Vec<u8>, mut v: usize) {
    loop {
        let mut b = (v & 0x7f) as u8;
        v >>= 7;
        if v != 0 {
            b |= 0x80;
        }
        out.push(b);
        if v == 0 {
            break;
        }
    }
}

/// Compile a legacy message. `payer` is the single signer and fee payer.
pub fn compile_message(payer: &Pk, ixs: &[Instruction], blockhash: &[u8; 32]) -> Vec<u8> {
    // collect unique keys with merged flags
    let mut keys: Vec<(Pk, bool, bool)> = vec![(*payer, true, true)];
    let upsert = |pk: Pk, s: bool, w: bool, keys: &mut Vec<(Pk, bool, bool)>| {
        if let Some(k) = keys.iter_mut().find(|k| k.0 == pk) {
            k.1 |= s;
            k.2 |= w;
        } else {
            keys.push((pk, s, w));
        }
    };
    for ix in ixs {
        for a in &ix.accounts {
            upsert(a.pk, a.signer, a.writable, &mut keys);
        }
        upsert(ix.program, false, false, &mut keys);
    }
    let payer_entry = keys.remove(0);
    let mut sw: Vec<Pk> = vec![payer_entry.0];
    let mut sr = Vec::new();
    let mut nw = Vec::new();
    let mut nr = Vec::new();
    for (k, s, w) in keys {
        match (s, w) {
            (true, true) => sw.push(k),
            (true, false) => sr.push(k),
            (false, true) => nw.push(k),
            (false, false) => nr.push(k),
        }
    }
    let ordered: Vec<Pk> = sw.iter().chain(sr.iter()).chain(nw.iter()).chain(nr.iter()).copied().collect();
    let idx = |p: &Pk| ordered.iter().position(|k| k == p).unwrap() as u8;
    let mut m = Vec::with_capacity(1024);
    m.push((sw.len() + sr.len()) as u8);
    m.push(sr.len() as u8);
    m.push(nr.len() as u8);
    compact_u16(&mut m, ordered.len());
    for k in &ordered {
        m.extend_from_slice(&k.0);
    }
    m.extend_from_slice(blockhash);
    compact_u16(&mut m, ixs.len());
    for ix in ixs {
        m.push(idx(&ix.program));
        compact_u16(&mut m, ix.accounts.len());
        for a in &ix.accounts {
            m.push(idx(&a.pk));
        }
        compact_u16(&mut m, ix.data.len());
        m.extend_from_slice(&ix.data);
    }
    m
}

/// Sign and serialize. With `key == None` a zero signature is used (simulation only).
pub fn build_tx(payer: &Pk, ixs: &[Instruction], blockhash: &[u8; 32], key: Option<&SigningKey>) -> (Vec<u8>, Sig) {
    let msg = compile_message(payer, ixs, blockhash);
    let sig = match key {
        Some(k) => Sig(k.sign(&msg).to_bytes()),
        None => Sig::default(),
    };
    let mut tx = Vec::with_capacity(msg.len() + 65);
    compact_u16(&mut tx, 1);
    tx.extend_from_slice(&sig.0);
    tx.extend_from_slice(&msg);
    (tx, sig)
}

pub const MAX_TX_SIZE: usize = 1232;

// ------------------------------------------------------------------ common instructions

pub fn cu_limit(units: u32) -> Instruction {
    let mut d = vec![2u8];
    d.extend_from_slice(&units.to_le_bytes());
    Instruction { program: COMPUTE_BUDGET, accounts: vec![], data: d }
}
pub fn cu_price(micro_lamports: u64) -> Instruction {
    let mut d = vec![3u8];
    d.extend_from_slice(&micro_lamports.to_le_bytes());
    Instruction { program: COMPUTE_BUDGET, accounts: vec![], data: d }
}
pub fn transfer(from: &Pk, to: &Pk, lamports: u64) -> Instruction {
    let mut d = 2u32.to_le_bytes().to_vec();
    d.extend_from_slice(&lamports.to_le_bytes());
    Instruction { program: SYSTEM_PROGRAM, accounts: vec![Meta::ws(*from), Meta::w(*to)], data: d }
}
pub fn create_ata_idempotent(payer: &Pk, owner: &Pk, mint: &Pk, token_program: &Pk) -> Instruction {
    let ata = Pk::ata(owner, mint, token_program);
    Instruction {
        program: ATA_PROGRAM,
        accounts: vec![Meta::ws(*payer), Meta::w(ata), Meta::r(*owner), Meta::r(*mint), Meta::r(SYSTEM_PROGRAM), Meta::r(*token_program)],
        data: vec![1],
    }
}
/// SPL / Token-2022 CloseAccount (returns the ATA rent to `dest`).
pub fn close_account(account: &Pk, dest: &Pk, owner: &Pk, token_program: &Pk) -> Instruction {
    Instruction {
        program: *token_program,
        accounts: vec![Meta::w(*account), Meta::w(*dest), Meta { pk: *owner, signer: true, writable: false }],
        data: vec![9],
    }
}

// ------------------------------------------------------------------ pump.fun v3

pub struct PumpAccounts {
    pub bonding_curve: Pk,
    pub assoc_base_curve: Pk,
    pub assoc_quote_curve: Pk,
    pub assoc_base_user: Pk,
    pub assoc_quote_user: Pk,
    pub user_volume_acc: Pk,
}

pub fn pump_accounts(user: &Pk, mint: &Pk, base_token_program: &Pk) -> PumpAccounts {
    let (bc, _) = Pk::find_pda(&[b"bonding-curve", &mint.0], &PUMP_PROGRAM);
    PumpAccounts {
        bonding_curve: bc,
        assoc_base_curve: Pk::ata(&bc, mint, base_token_program),
        assoc_quote_curve: Pk::ata(&bc, &WSOL, &TOKEN_PROGRAM),
        assoc_base_user: Pk::ata(user, mint, base_token_program),
        assoc_quote_user: Pk::ata(user, &WSOL, &TOKEN_PROGRAM),
        user_volume_acc: Pk::find_pda(&[b"user_volume_accumulator", &user.0], &PUMP_PROGRAM).0,
    }
}

fn v3_accounts(user: &Pk, mint: &Pk, base_token_program: &Pk, buyback: &Pk) -> Vec<Meta> {
    let a = pump_accounts(user, mint, base_token_program);
    vec![
        Meta::r(GLOBAL),
        Meta::r(*mint),
        Meta::r(WSOL),
        Meta::r(*base_token_program),
        Meta::r(TOKEN_PROGRAM),
        Meta::w(a.bonding_curve),
        Meta::w(a.assoc_base_curve),
        Meta::w(a.assoc_quote_curve),
        Meta::ws(*user),
        Meta::w(a.assoc_base_user),
        Meta::w(a.assoc_quote_user),
        Meta::w(a.user_volume_acc),
        Meta::r(FEE_CONFIG),
        Meta::w(*buyback),
        Meta::r(SYSTEM_PROGRAM),
        Meta::r(EVENT_AUTHORITY),
        Meta::r(PUMP_PROGRAM),
    ]
}

pub fn pump_buy_exact_quote_in_v3(user: &Pk, mint: &Pk, base_token_program: &Pk, spend_lamports: u64, min_tokens_out: u64, buyback: &Pk) -> Instruction {
    let mut d = IX_BUY_EXACT_QUOTE_IN_V3.to_vec();
    d.extend_from_slice(&spend_lamports.to_le_bytes());
    d.extend_from_slice(&min_tokens_out.to_le_bytes());
    Instruction { program: PUMP_PROGRAM, accounts: v3_accounts(user, mint, base_token_program, buyback), data: d }
}

pub fn pump_sell_v3(user: &Pk, mint: &Pk, base_token_program: &Pk, amount: u64, min_sol_out: u64, buyback: &Pk) -> Instruction {
    let mut d = IX_SELL_V3.to_vec();
    d.extend_from_slice(&amount.to_le_bytes());
    d.extend_from_slice(&min_sol_out.to_le_bytes());
    Instruction { program: PUMP_PROGRAM, accounts: v3_accounts(user, mint, base_token_program, buyback), data: d }
}

pub struct TxParams {
    pub cu_limit: u32,
    pub cu_price: u64,
    pub tip_lamports: u64,
    pub tip_account: Option<Pk>,
}

/// Full buy transaction instruction list: budget, ATA (idempotent), buy, tip.
pub fn buy_ixs(user: &Pk, mint: &Pk, token_program: &Pk, spend: u64, min_tokens: u64, p: &TxParams, buyback_idx: usize) -> Vec<Instruction> {
    let mut v = vec![cu_limit(p.cu_limit), cu_price(p.cu_price), create_ata_idempotent(user, user, mint, token_program)];
    v.push(pump_buy_exact_quote_in_v3(user, mint, token_program, spend, min_tokens, &BUYBACK_FEE_RECIPIENTS[buyback_idx % 8]));
    if let (Some(t), true) = (p.tip_account, p.tip_lamports > 0) {
        v.push(transfer(user, &t, p.tip_lamports));
    }
    v
}

/// Full sell transaction: budget, sell, optional ATA close (when selling everything), tip.
pub fn sell_ixs(user: &Pk, mint: &Pk, token_program: &Pk, amount: u64, min_out: u64, close: bool, p: &TxParams, buyback_idx: usize) -> Vec<Instruction> {
    let mut v = vec![cu_limit(p.cu_limit), cu_price(p.cu_price)];
    v.push(pump_sell_v3(user, mint, token_program, amount, min_out, &BUYBACK_FEE_RECIPIENTS[buyback_idx % 8]));
    if close {
        let ata = Pk::ata(user, mint, token_program);
        v.push(close_account(&ata, user, user, token_program));
    }
    if let (Some(t), true) = (p.tip_account, p.tip_lamports > 0) {
        v.push(transfer(user, &t, p.tip_lamports));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn buy_tx_fits() {
        let k = SigningKey::from_bytes(&[7u8; 32]);
        let user = Pk(k.verifying_key().to_bytes());
        let mint = crate::pk::pk("GyXwHm4gk8a5PAhAo6Nj1dJuhnRjTujst6GSGUYzpump");
        let p = TxParams { cu_limit: 120_000, cu_price: 1_000_000, tip_lamports: 1_000_000, tip_account: Some(crate::tx::TIP_ACCOUNTS[8]) };
        let ixs = buy_ixs(&user, &mint, &TOKEN_2022, 100_000_000, 1, &p, 0);
        let (tx, sig) = build_tx(&user, &ixs, &[1u8; 32], Some(&k));
        assert!(tx.len() <= MAX_TX_SIZE, "tx is {} bytes", tx.len());
        // signature verifies over the message
        let msg = &tx[65..];
        use ed25519_dalek::Verifier;
        k.verifying_key().verify(msg, &ed25519_dalek::Signature::from_bytes(&sig.0)).unwrap();
    }
}
