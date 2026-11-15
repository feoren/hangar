//! Model prices for the cost estimate (`cost_service.rs`): a small table in US dollars per million
//! tokens, built in (`model_prices.toml`) and extended or overridden by the user's
//! `model-prices.toml` next to profiles.toml. A model without an entry has no price; the window
//! then shows its tokens only.

use std::path::Path;

use serde::Deserialize;
use thiserror::Error;

use crate::token_usage::Tokens;

/// The built-in table, with its source and the date it was taken.
pub const BUILT_IN: &str = include_str!("model_prices.toml");
/// The user's own table in the config folder.
pub const USER_FILE: &str = "model-prices.toml";
/// Larger user files are not read: a price table has a few dozen lines.
const MAX_USER_BYTES: u64 = 256 * 1024;

const PER_MILLION: f64 = 1_000_000.0;
/// Cache write prices relative to the input price when an entry names none: 5 minutes and 1 hour.
const WRITE_5M_FACTOR: f64 = 1.25;
const WRITE_1H_FACTOR: f64 = 2.0;
/// Cache read price relative to the input price when an entry names none.
const READ_FACTOR: f64 = 0.1;

#[derive(Debug, Error)]
pub enum PriceError {
    #[error("price table: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("price table: {0}")]
    Invalid(String),
    #[error("price table: {0}")]
    Io(#[from] std::io::Error),
}

/// Prices of one model in dollars per million tokens.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelPrice {
    pub id: String,
    pub input: f64,
    pub output: f64,
    pub cache_write_5m: Option<f64>,
    pub cache_write_1h: Option<f64>,
    pub cache_read: Option<f64>,
    /// Factor on every price for a fast mode response; without one such a response has no price.
    pub fast_factor: Option<f64>,
    /// From this many prompt tokens on, `long_input` and `long_output` apply.
    pub long_above: Option<u64>,
    pub long_input: Option<f64>,
    pub long_output: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PriceTable {
    /// Where the prices come from, shown under the cost overview.
    #[serde(default)]
    pub source: String,
    /// When they were taken (YYYY-MM-DD).
    #[serde(default)]
    pub retrieved: String,
    #[serde(default, rename = "model")]
    pub models: Vec<ModelPrice>,
}

fn valid_price(value: f64) -> bool {
    value.is_finite() && value >= 0.0
}

impl ModelPrice {
    fn check(&self) -> Result<(), PriceError> {
        let id = self.id.trim();
        if id.is_empty() || id.len() > 128 {
            return Err(PriceError::Invalid("a model needs an id".into()));
        }
        let prices = [
            Some(self.input),
            Some(self.output),
            self.cache_write_5m,
            self.cache_write_1h,
            self.cache_read,
            self.fast_factor,
            self.long_input,
            self.long_output,
        ];
        if !prices.into_iter().flatten().all(valid_price) {
            return Err(PriceError::Invalid(format!(
                "{id}: prices must be numbers of 0 or more"
            )));
        }
        let long = [
            self.long_above.is_some(),
            self.long_input.is_some(),
            self.long_output.is_some(),
        ];
        if long.iter().any(|set| *set) && !long.iter().all(|set| *set) {
            return Err(PriceError::Invalid(format!(
                "{id}: long_above, long_input and long_output belong together"
            )));
        }
        Ok(())
    }

    /// Whether this entry prices responses of `model`: the same id, or the id followed by a date
    /// (`-20251101`) or an option in brackets (`[1m]`). A longer id that only starts with this one
    /// (`claude-opus-5-5` for `claude-opus-5`) is another model.
    fn matches(&self, model: &str) -> bool {
        let model = model.trim().to_ascii_lowercase();
        let id = self.id.trim().to_ascii_lowercase();
        let Some(rest) = model.strip_prefix(&id) else {
            return false;
        };
        let (rest, option) = match rest.find('[') {
            Some(at) => (&rest[..at], &rest[at..]),
            None => (rest, ""),
        };
        let dated = rest.is_empty()
            || rest
                .strip_prefix('-')
                .is_some_and(|date| date.len() == 8 && date.bytes().all(|b| b.is_ascii_digit()));
        dated && (option.is_empty() || option.ends_with(']'))
    }

    /// Dollars for one response, `None` for a fast response without `fast_factor`.
    pub fn cost(&self, tokens: &Tokens, fast: bool) -> Option<f64> {
        let factor = if fast { self.fast_factor? } else { 1.0 };
        let prompt = tokens
            .input
            .saturating_add(tokens.cache_write_5m)
            .saturating_add(tokens.cache_write_1h)
            .saturating_add(tokens.cache_read);
        let long = self.long_above.is_some_and(|above| prompt > above);
        let (input, output) = match (long, self.long_input, self.long_output) {
            (true, Some(input), Some(output)) => (input, output),
            _ => (self.input, self.output),
        };
        // An explicit cache price belongs to the normal tier; above it the factors apply.
        let explicit = |price: Option<f64>| if long { None } else { price };
        let write_5m = explicit(self.cache_write_5m).unwrap_or(input * WRITE_5M_FACTOR);
        let write_1h = explicit(self.cache_write_1h).unwrap_or(input * WRITE_1H_FACTOR);
        let read = explicit(self.cache_read).unwrap_or(input * READ_FACTOR);
        #[allow(clippy::cast_precision_loss)] // Token counts stay far below 2^52.
        let dollars = (tokens.input as f64 * input
            + tokens.output as f64 * output
            + tokens.cache_write_5m as f64 * write_5m
            + tokens.cache_write_1h as f64 * write_1h
            + tokens.cache_read as f64 * read)
            / PER_MILLION;
        Some(dollars * factor)
    }
}

/// The user's price file as text, `None` when it holds more than [`MAX_USER_BYTES`]. The limit
/// applies to what is read, not to a size looked up before: the file may grow in between.
fn read_user_file(path: &Path) -> std::io::Result<Option<String>> {
    use std::io::Read as _;
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(MAX_USER_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_USER_BYTES {
        return Ok(None);
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
}

impl PriceTable {
    pub fn parse(text: &str) -> Result<Self, PriceError> {
        let table: Self = toml::from_str(text)?;
        for model in &table.models {
            model.check()?;
        }
        Ok(table)
    }

    /// The built-in table. It is checked by a test, so a broken one never ships; should it fail
    /// anyway, no model has a price.
    pub fn built_in() -> Self {
        Self::parse(BUILT_IN).unwrap_or_default()
    }

    /// The built-in table with the user's file in `config_dir` on top: an entry with the same id
    /// replaces the built-in one, new ids are added. A missing file is no error; a broken one is
    /// reported and left out entirely, so a typo never prices with half a table.
    pub fn load(config_dir: &Path) -> (Self, Option<PriceError>) {
        let mut table = Self::built_in();
        let path = config_dir.join(USER_FILE);
        let user = match read_user_file(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return (table, None),
            Err(error) => Err(PriceError::Io(error)),
            Ok(None) => Err(PriceError::Invalid(format!("{USER_FILE} is too large"))),
            Ok(Some(text)) => Self::parse(&text),
        };
        match user {
            Ok(user) => {
                table.merge(user);
                (table, None)
            }
            Err(error) => (table, Some(error)),
        }
    }

    fn merge(&mut self, user: Self) {
        if !user.source.trim().is_empty() {
            self.source = user.source;
            self.retrieved = user.retrieved;
        }
        for model in user.models {
            let id = model.id.trim().to_ascii_lowercase();
            self.models
                .retain(|known| known.id.trim().to_ascii_lowercase() != id);
            self.models.push(model);
        }
    }

    /// The entry for `model`, the longest matching id first.
    pub fn lookup(&self, model: &str) -> Option<&ModelPrice> {
        self.models
            .iter()
            .filter(|price| price.matches(model))
            .max_by_key(|price| price.id.trim().len())
    }

    /// Dollars for one response of `model`; `None` when the model has no price.
    pub fn cost(&self, model: &str, tokens: &Tokens, fast: bool) -> Option<f64> {
        self.lookup(model)?.cost(tokens, fast)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::test_support::TempDir;

    fn tokens(input: u64, output: u64, write_5m: u64, write_1h: u64, read: u64) -> Tokens {
        Tokens {
            input,
            output,
            cache_write_5m: write_5m,
            cache_write_1h: write_1h,
            cache_read: read,
        }
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn the_built_in_table_parses_and_names_its_source() {
        let table = PriceTable::parse(BUILT_IN).unwrap();
        assert!(!table.source.is_empty());
        assert_eq!(table.retrieved.len(), 10);
        assert!(table.models.len() >= 10);
        assert_eq!(PriceTable::built_in(), table);
    }

    #[test]
    fn ids_match_exactly_or_with_a_date_or_option() {
        let table = PriceTable::built_in();
        let id = |model: &str| table.lookup(model).map(|price| price.id.as_str());
        assert_eq!(id("claude-opus-5-5"), Some("claude-opus-5-5"));
        assert_eq!(id("claude-opus-5"), Some("claude-opus-5"));
        assert_eq!(id("Claude-Opus-5-5[1m]"), Some("claude-opus-5-5"));
        assert_eq!(id("claude-haiku-4-5-20251001"), Some("claude-haiku-4-5"));
        assert_eq!(id("claude-fable-5-2"), Some("claude-fable-5-2"));
        // A newer model is not priced like its predecessor.
        assert_eq!(id("claude-opus-5-7"), None);
        assert_eq!(id("claude-opus-4-5"), None);
        assert_eq!(id("claude-opus-5-5-fast"), None);
        assert_eq!(id("<synthetic>"), None);
        assert_eq!(id(""), None);
    }

    #[test]
    fn costs_use_the_cache_prices_and_factors() {
        let table = PriceTable::built_in();
        // Opus 5.5: $4 in, $20 out, $5 / $8 writes, $0.20 reads per million.
        let used = tokens(1_000_000, 100_000, 200_000, 100_000, 2_000_000);
        let cost = table.cost("claude-opus-5-5", &used, false).unwrap();
        assert!(close(cost, 4.0 + 2.0 + 1.0 + 0.8 + 0.4), "{cost}");
        assert!(close(
            table.cost("claude-opus-5-5", &used, true).unwrap(),
            cost * 2.0
        ));
        // Opus 5 names no cache prices: 1.25x, 2x and 0.1x of $5.
        let cost = table
            .cost(
                "claude-opus-5",
                &tokens(0, 0, 1_000_000, 1_000_000, 1_000_000),
                false,
            )
            .unwrap();
        assert!(close(cost, 6.25 + 10.0 + 0.5), "{cost}");
        // No fast price known, and no price for an unknown model.
        assert_eq!(table.cost("claude-opus-5", &used, true), None);
        assert_eq!(table.cost("gpt-9", &used, false), None);
    }

    #[test]
    fn a_long_prompt_switches_to_the_long_prices() {
        let table = PriceTable::built_in();
        let short = table
            .cost(
                "claude-haiku-5-5",
                &tokens(100_000, 1_000_000, 0, 0, 0),
                false,
            )
            .unwrap();
        assert!(close(short, 0.01 + 0.5), "{short}");
        let long = table
            .cost(
                "claude-haiku-5-5",
                &tokens(1, 1_000_000, 0, 0, 100_000),
                false,
            )
            .unwrap();
        assert!(close(long, 0.000_000_5 + 2.5 + 0.005), "{long}");
    }

    #[test]
    fn broken_tables_are_refused() {
        for text in [
            "[[model]]\nid = \"\"\ninput = 1.0\noutput = 1.0\n",
            "[[model]]\nid = \"m\"\ninput = -1.0\noutput = 1.0\n",
            "[[model]]\nid = \"m\"\ninput = 1.0\noutput = nan\n",
            "[[model]]\nid = \"m\"\ninput = 1.0\noutput = 1.0\nlong_above = 5\n",
            "[[model]]\nid = \"m\"\ninput = 1.0\noutput = 1.0\nprice = 3.0\n",
            "[[model]]\nid = \"m\"\ninput = 1.0\n",
            "not toml",
        ] {
            assert!(PriceTable::parse(text).is_err(), "{text}");
        }
    }

    #[test]
    fn the_user_file_adds_and_replaces_entries() {
        let dir = TempDir::new("model-prices");
        let (table, error) = PriceTable::load(dir.path());
        assert_eq!(table, PriceTable::built_in());
        assert!(error.is_none());
        fs::write(
            dir.path().join(USER_FILE),
            "source = \"own\"\nretrieved = \"2026-11-13\"\n\
             [[model]]\nid = \"claude-opus-5-5\"\ninput = 1.0\noutput = 2.0\n\
             [[model]]\nid = \"local-model\"\ninput = 0.0\noutput = 0.0\n",
        )
        .unwrap();
        let (table, error) = PriceTable::load(dir.path());
        assert!(error.is_none());
        assert_eq!(table.source, "own");
        assert_eq!(table.lookup("claude-opus-5-5").unwrap().input, 1.0);
        assert_eq!(table.lookup("local-model").unwrap().output, 0.0);
        assert!(table.lookup("claude-sonnet-5-5").is_some());
        // A broken file is reported and ignored as a whole.
        fs::write(dir.path().join(USER_FILE), "[[model]]\nid = 3\n").unwrap();
        let (table, error) = PriceTable::load(dir.path());
        assert!(error.is_some());
        assert_eq!(table, PriceTable::built_in());
    }

    #[test]
    fn a_user_file_over_the_limit_is_refused_without_reading_it_all() {
        let dir = TempDir::new("model-prices-limit");
        let entry = "[[model]]\nid = \"local-model\"\ninput = 0.0\noutput = 0.0\n";
        let padded = |len: usize| format!("{entry}{}", "#".repeat(len - entry.len()));
        let limit = usize::try_from(MAX_USER_BYTES).unwrap();
        fs::write(dir.path().join(USER_FILE), padded(limit)).unwrap();
        let (table, error) = PriceTable::load(dir.path());
        assert!(error.is_none(), "{error:?}");
        assert!(table.lookup("local-model").is_some());
        fs::write(dir.path().join(USER_FILE), padded(limit + 1)).unwrap();
        let (table, error) = PriceTable::load(dir.path());
        assert!(matches!(error, Some(PriceError::Invalid(_))), "{error:?}");
        assert_eq!(table, PriceTable::built_in());
    }
}
