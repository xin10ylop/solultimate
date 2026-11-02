//! Dump Meteora DBC events of one or more stores to CSV for research.
//! usage: dbcdump out.csv <dir> [<dir> ...]
use anyhow::Result;
use std::io::Write;
use ucore::dbc::{fee_rate, DbcKind};
use ucore::event::Ev;
use ucore::store::StoreReader;

fn main() -> Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let mut w = std::io::BufWriter::new(std::fs::File::create(&a[1])?);
    writeln!(w, "kind,slot,tx,ev,block_time,ts,pool,config,user,dir,amount_in,amount_out,fee,fee_on_base,fee_rate,quote_reserve,sqrt_price,base_mint,quote_mint,cu_price,tip,tx_fee")?;
    let mut n = 0u64;
    for d in &a[2..] {
        for blk in StoreReader::open(d)? {
            for e in &blk {
                if let Ev::Dbc(x) = e {
                    let k = match x.e.kind {
                        DbcKind::Swap => "S",
                        DbcKind::Init => "I",
                        DbcKind::Complete => "C",
                    };
                    writeln!(
                        w,
                        "{k},{},{},{},{},{},{},{},{},{},{},{},{},{},{:.6},{},{},{},{},{},{},{}",
                        x.m.slot, x.m.tx_idx, x.m.ev_idx, x.m.block_time, x.e.ts, x.e.pool, x.e.config, x.e.user, x.e.dir, x.e.amount_in, x.e.amount_out, x.e.fee,
                        x.e.fee_on_base as u8, fee_rate(&x.e), x.e.quote_reserve, x.e.sqrt_price, x.e.base_mint, x.e.quote_mint, x.m.cu_price, x.m.tip, x.m.tx_fee
                    )?;
                    n += 1;
                }
            }
        }
    }
    eprintln!("dbc events {n}");
    Ok(())
}
