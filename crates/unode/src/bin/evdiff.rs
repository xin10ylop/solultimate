//! Compare two event stores over a slot range, block by block (debug-string multisets).
use anyhow::Result;
use std::collections::BTreeMap;
use ucore::event::Ev;
use ucore::store::StoreReader;

fn load(dir: &str, a: u64, b: u64) -> Result<BTreeMap<u64, Vec<String>>> {
    let mut m = BTreeMap::new();
    for blk in StoreReader::open(dir)? {
        let s = blk.last().map(|e| e.slot()).unwrap_or(0);
        if s < a || s >= b {
            continue;
        }
        let mut v: Vec<String> = blk.iter().map(|e| format!("{e:?}")).collect();
        v.sort();
        m.insert(s, v);
    }
    Ok(m)
}

fn kind(s: &str) -> &str {
    s.split('(').next().unwrap_or("?")
}

fn main() -> Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let (x, y, lo, hi) = (&a[1], &a[2], a[3].parse::<u64>()?, a[4].parse::<u64>()?);
    let (ma, mb) = (load(x, lo, hi)?, load(y, lo, hi)?);
    if let Some(pat) = a.get(5) {
        for (name, m) in [("A", &ma), ("B", &mb)] {
            for (s, v) in m.iter() {
                for e in v {
                    if e.contains(pat.as_str()) {
                        println!("{name} {s}: {e}");
                    }
                }
            }
        }
        return Ok(());
    }
    println!("blocks: {} vs {}", ma.len(), mb.len());
    let mut cnt: BTreeMap<String, (u64, u64, u64)> = BTreeMap::new();
    let mut shown = 0;
    for (s, va) in &ma {
        let Some(vb) = mb.get(s) else { continue };
        for e in va {
            cnt.entry(kind(e).to_string()).or_default().0 += 1;
        }
        for e in vb {
            cnt.entry(kind(e).to_string()).or_default().1 += 1;
        }
        let sb: std::collections::HashSet<&String> = vb.iter().collect();
        for e in va {
            if sb.contains(e) {
                cnt.entry(kind(e).to_string()).or_default().2 += 1;
            } else if shown < 6 && !e.starts_with("Block") {
                println!("only in A slot {s}: {}", &e[..e.len().min(400)]);
                shown += 1;
            }
        }
    }
    for (k, (na, nb, both)) in cnt {
        println!("{k:10} A {na:8} B {nb:8} identical {both:8}");
    }
    let only_a: Vec<_> = ma.keys().filter(|s| !mb.contains_key(s)).take(5).collect();
    let only_b: Vec<_> = mb.keys().filter(|s| !ma.contains_key(s)).take(5).collect();
    println!("blocks only in A (first 5): {only_a:?}  only in B: {only_b:?}");
    Ok(())
}
