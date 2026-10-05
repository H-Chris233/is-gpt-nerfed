//! ModelTrace (MIT, xqy2006): numerically equivalent to the reference scorer.
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::LazyLock};

const DIM: usize = 355;
static NUMBERS: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"\p{Nd}+").unwrap());

#[derive(Deserialize)]
pub struct Bank {
    models: Vec<Model>,
    robust: Robust,
    calibration: BTreeMap<String, Calibration>,
}
#[derive(Deserialize)]
struct Model {
    id: String,
    counts: Vec<f64>,
}
#[derive(Deserialize)]
struct Robust {
    hellinger: Artifact,
    ordered_blocks: Option<Artifact>,
}
#[derive(Deserialize)]
struct Artifact {
    feature_mean: Vec<f64>,
    feature_scale: Vec<f64>,
    nuisance_basis: Vec<Vec<f64>>,
    centroids: Vec<Vec<f64>>,
    #[serde(default)]
    environment_centroids: Vec<Vec<Vec<f64>>>,
    #[serde(default)]
    weight: f64,
}
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Calibration {
    pub beta: f64,
    pub cv_accuracy: f64,
}
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Output {
    pub text: String,
    pub expected_count: usize,
}
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Candidate {
    pub model: String,
    pub probability: f64,
    pub score: f64,
    pub profile_similarity: f64,
}
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Diagnostic {
    pub parsed_numbers: usize,
    pub minimum_numbers: usize,
    pub accepted: bool,
}
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Analysis {
    pub prediction: String,
    pub probability: f64,
    pub used_outputs: usize,
    pub results: Vec<Candidate>,
    pub diagnostics: Vec<Diagnostic>,
    pub calibration: Calibration,
}

impl Bank {
    pub fn embedded() -> Result<Self> {
        let bank: Self = serde_json::from_str(include_str!(
            "../plugin/assets/modeltrace/unified_bank.json"
        ))?;
        bank.validate()?;
        Ok(bank)
    }
    fn validate(&self) -> Result<()> {
        if self.models.is_empty() || self.models.iter().any(|m| m.counts.len() != DIM) {
            bail!("invalid fingerprint bank");
        }
        for (a, dimension) in [(&self.robust.hellinger, DIM)]
            .into_iter()
            .chain(self.robust.ordered_blocks.as_ref().map(|a| (a, 74)))
        {
            if a.feature_mean.len() != dimension
                || a.feature_scale.len() != dimension
                || a.feature_scale.iter().any(|v| !v.is_finite() || *v <= 0.0)
                || a.centroids.len() != self.models.len()
                || a.centroids
                    .iter()
                    .chain(a.nuisance_basis.iter())
                    .any(|v| v.len() != dimension)
                || a.environment_centroids.iter().any(|env| {
                    env.len() != self.models.len() || env.iter().any(|v| v.len() != dimension)
                })
            {
                bail!("invalid fingerprint dimensions");
            }
        }
        Ok(())
    }
}

pub fn parse_numbers(text: &str) -> Vec<usize> {
    let mut longest = Vec::new();
    let mut current = Vec::new();
    let mut previous = 0;
    for m in NUMBERS.find_iter(text) {
        if !current.is_empty() && text[previous..m.start()].chars().any(char::is_alphabetic) {
            if current.len() > longest.len() {
                longest = current.clone();
            }
            current.clear();
        }
        // Unicode decimal digits are accepted by Python's int as well.
        let ascii: String = m
            .as_str()
            .chars()
            .map(|c| unicode_digit(c).map(|d| (b'0' + d) as char).unwrap_or(c))
            .collect();
        if let Ok(n @ 1..=DIM) = ascii.parse::<usize>() {
            current.push(n);
        }
        previous = m.end();
    }
    if current.len() > longest.len() {
        longest = current;
    }
    longest
}

fn unicode_digit(c: char) -> Option<u8> {
    const STARTS: &[u32] = &[
        0x30, 0x660, 0x6f0, 0x7c0, 0x966, 0x9e6, 0xa66, 0xae6, 0xb66, 0xbe6, 0xc66, 0xce6, 0xd66,
        0xde6, 0xe50, 0xed0, 0xf20, 0x1040, 0x1090, 0x17e0, 0x1810, 0x1946, 0x19d0, 0x1a80, 0x1a90,
        0x1b50, 0x1bb0, 0x1c40, 0x1c50, 0xa620, 0xa8d0, 0xa900, 0xa9d0, 0xa9f0, 0xaa50, 0xabf0,
        0xff10, 0x104a0, 0x10d30, 0x11066, 0x110f0, 0x11136, 0x111d0, 0x112f0, 0x11450, 0x114d0,
        0x11650, 0x116c0, 0x11730, 0x118e0, 0x11950, 0x11c50, 0x11d50, 0x11da0, 0x16a60, 0x16ac0,
        0x16b50, 0x1d7ce, 0x1d7d8, 0x1d7e2, 0x1d7ec, 0x1d7f6, 0x1e140, 0x1e2f0, 0x1e950,
    ];
    let v = c as u32;
    STARTS
        .iter()
        .find_map(|start| (v >= *start && v < start + 10).then(|| (v - start) as u8))
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(a, b)| a * b).sum()
}
fn normalized(mut v: Vec<f64>) -> Vec<f64> {
    let norm = dot(&v, &v).sqrt().max(1e-12);
    for x in &mut v {
        *x /= norm;
    }
    v
}
fn standardize(mut v: Vec<f64>) -> Vec<f64> {
    let center = v.iter().sum::<f64>() / v.len() as f64;
    let scale = (v.iter().map(|x| (x - center).powi(2)).sum::<f64>() / v.len() as f64)
        .sqrt()
        .max(1e-12);
    for x in &mut v {
        *x = (*x - center) / scale;
    }
    v
}
fn project(mut v: Vec<f64>, a: &Artifact) -> Vec<f64> {
    for basis in &a.nuisance_basis {
        let projection = dot(&v, basis);
        for (x, b) in v.iter_mut().zip(basis) {
            *x -= projection * b;
        }
    }
    normalized(v)
}
fn scale(feature: &[f64], a: &Artifact) -> Vec<f64> {
    feature
        .iter()
        .zip(&a.feature_mean)
        .zip(&a.feature_scale)
        .map(|((v, m), s)| (v - m) / s)
        .collect()
}
fn counts(numbers: &[usize]) -> Vec<f64> {
    let mut result = vec![0.0; DIM];
    for n in numbers {
        result[n - 1] += 1.0;
    }
    result
}
fn scores(numbers: &[usize], bank: &Bank) -> Vec<f64> {
    let count = counts(numbers);
    let total = numbers.len() as f64 + 0.5 * DIM as f64;
    let feature: Vec<_> = count.iter().map(|v| ((v + 0.5) / total).sqrt()).collect();
    let a = &bank.robust.hellinger;
    let unit = project(scale(&feature, a), a);
    let marginal = standardize(a.centroids.iter().map(|c| dot(&unit, c)).collect());
    let Some(a) = &bank.robust.ordered_blocks else {
        return marginal;
    };
    if a.weight == 0.0 {
        return marginal;
    }
    let mut feature = Vec::with_capacity(74);
    let mut start = 0;
    for i in 0..4 {
        let size = numbers.len() / 4 + usize::from(i < numbers.len() % 4);
        let mut bins = [0.5; 16];
        for n in &numbers[start..start + size] {
            bins[(((*n - 1) as f64 / 355.0) * 16.0) as usize] += 1.0;
        }
        let total: f64 = bins.iter().sum();
        feature.extend(bins.iter().map(|v| (v / total).sqrt()));
        start += size;
    }
    let mut digits = [0.5; 10];
    for n in numbers {
        digits[n % 10] += 1.0;
    }
    let total: f64 = digits.iter().sum();
    feature.extend(digits.iter().map(|v| (v / total).sqrt()));
    let scaled = scale(&feature, a);
    let unit = normalized(scaled.clone());
    let template = standardize(
        (0..bank.models.len())
            .map(|i| {
                a.environment_centroids
                    .iter()
                    .map(|e| dot(&unit, &e[i]))
                    .fold(f64::NEG_INFINITY, f64::max)
            })
            .collect(),
    );
    let projected = project(scaled, a);
    let nuisance = standardize(a.centroids.iter().map(|c| dot(&projected, c)).collect());
    let ordered = standardize(
        template
            .iter()
            .zip(nuisance)
            .map(|(t, n)| 0.5 * t + 0.5 * n)
            .collect(),
    );
    marginal
        .iter()
        .zip(ordered)
        .map(|(m, o)| (1.0 - a.weight) * m + a.weight * o)
        .collect()
}
fn similarity(left: &[f64], right: &[f64]) -> f64 {
    let lt: f64 = left.iter().sum();
    let rt = right.iter().sum::<f64>() + 0.5 * DIM as f64;
    let divergence: f64 = left
        .iter()
        .zip(right)
        .map(|(l, r)| {
            let p = l / lt;
            let q = (r + 0.5) / rt;
            let m = (p + q) / 2.0;
            (if p > 0.0 { p * (p / m).ln() } else { 0.0 }) + q * (q / m).ln()
        })
        .sum();
    1.0 - (divergence / 2.0 / std::f64::consts::LN_2).sqrt()
}
pub fn analyze(outputs: &[Output], bank: &Bank) -> Result<Analysis> {
    let mut valid = Vec::new();
    let mut diagnostics = Vec::new();
    for output in outputs {
        let numbers = parse_numbers(&output.text);
        let minimum = 80.max((output.expected_count as f64 * 0.55).ceil() as usize);
        let accepted = numbers.len() >= minimum;
        diagnostics.push(Diagnostic {
            parsed_numbers: numbers.len(),
            minimum_numbers: minimum,
            accepted,
        });
        if accepted {
            valid.push((scores(&numbers, bank), counts(&numbers)));
        }
    }
    if valid.is_empty() {
        bail!("no usable answer: every response was refused or badly truncated");
    }
    let combined: Vec<f64> = (0..bank.models.len())
        .map(|i| valid.iter().map(|v| v.0[i]).sum::<f64>() / valid.len() as f64)
        .collect();
    let calibration = bank
        .calibration
        .get(&valid.len().min(3).to_string())
        .ok_or_else(|| anyhow::anyhow!("missing calibration"))?
        .clone();
    let logits: Vec<f64> = combined.iter().map(|s| calibration.beta * s).collect();
    let max = logits.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let weights: Vec<f64> = logits.iter().map(|x| (x - max).exp()).collect();
    let total: f64 = weights.iter().sum();
    let pooled: Vec<f64> = (0..DIM)
        .map(|i| valid.iter().map(|v| v.1[i]).sum())
        .collect();
    let mut results: Vec<Candidate> = bank
        .models
        .iter()
        .enumerate()
        .map(|(i, m)| Candidate {
            model: m.id.clone(),
            probability: weights[i] / total,
            score: combined[i],
            profile_similarity: similarity(&pooled, &m.counts),
        })
        .collect();
    results.sort_by(|a, b| b.probability.total_cmp(&a.probability));
    Ok(Analysis {
        prediction: results[0].model.clone(),
        probability: results[0].probability,
        used_outputs: valid.len(),
        results,
        diagnostics,
        calibration,
    })
}
