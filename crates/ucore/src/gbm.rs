//! Minimal LightGBM model evaluator (from `Booster.dump_model()` JSON). Lets the identity-free
//! flow model be trained offline in Python and scored in Rust in microseconds.

use anyhow::{anyhow, Result};
use serde_json::Value;

#[derive(Clone, Debug)]
enum Node {
    Split { feature: usize, threshold: f64, default_left: bool, left: Box<Node>, right: Box<Node> },
    Leaf(f64),
}

#[derive(Clone, Debug)]
pub struct Gbm {
    trees: Vec<Node>,
    pub feature_names: Vec<String>,
}

fn parse_node(v: &Value) -> Result<Node> {
    if let Some(l) = v.get("leaf_value") {
        return Ok(Node::Leaf(l.as_f64().ok_or_else(|| anyhow!("bad leaf"))?));
    }
    let dt = v["decision_type"].as_str().unwrap_or("<=");
    if dt != "<=" {
        return Err(anyhow!("unsupported decision_type {dt} (categorical splits are not used)"));
    }
    Ok(Node::Split {
        feature: v["split_feature"].as_u64().ok_or_else(|| anyhow!("no split_feature"))? as usize,
        threshold: v["threshold"].as_f64().ok_or_else(|| anyhow!("no threshold"))?,
        default_left: v["default_left"].as_bool().unwrap_or(true),
        left: Box::new(parse_node(&v["left_child"])?),
        right: Box::new(parse_node(&v["right_child"])?),
    })
}

impl Gbm {
    pub fn from_json(v: &Value) -> Result<Gbm> {
        let mut trees = Vec::new();
        for t in v["tree_info"].as_array().ok_or_else(|| anyhow!("no tree_info"))? {
            trees.push(parse_node(&t["tree_structure"])?);
        }
        let feature_names = v["feature_names"]
            .as_array()
            .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
            .unwrap_or_default();
        Ok(Gbm { trees, feature_names })
    }

    pub fn load(path: &str, label: &str) -> Result<Gbm> {
        let v: Value = serde_json::from_str(&std::fs::read_to_string(path)?)?;
        let m = v.get(label).ok_or_else(|| anyhow!("label {label} not in {path}"))?;
        let g = Gbm::from_json(m)?;
        // the model must have been trained on exactly our feature layout
        let ours: Vec<String> = crate::flow::FEATURE_NAMES.iter().map(|s| s.to_string()).collect();
        if !g.feature_names.is_empty() && g.feature_names != ours {
            return Err(anyhow!("model features {:?} do not match engine features", g.feature_names));
        }
        Ok(g)
    }

    pub fn predict(&self, x: &[f32]) -> f64 {
        let mut s = 0.0;
        for t in &self.trees {
            let mut n = t;
            loop {
                match n {
                    Node::Leaf(v) => {
                        s += v;
                        break;
                    }
                    Node::Split { feature, threshold, default_left, left, right } => {
                        let f = x.get(*feature).copied().unwrap_or(f32::NAN) as f64;
                        let go_left = if f.is_nan() { *default_left } else { f <= *threshold };
                        n = if go_left { left } else { right };
                    }
                }
            }
        }
        s
    }

    pub fn n_trees(&self) -> usize {
        self.trees.len()
    }
}
