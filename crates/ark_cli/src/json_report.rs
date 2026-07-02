use std::collections::BTreeMap;
use std::io::Write;

use serde::Serialize;
use serde_json::{json, Map, Value};

use crate::bench_contract::{self, Evaluation, MetricValue};

pub fn write_json_line<W, T>(mut writer: W, report: &T) -> Result<(), String>
where
    W: Write,
    T: Serialize,
{
    serde_json::to_writer(&mut writer, report).map_err(|err| err.to_string())?;
    writer.write_all(b"\n").map_err(|err| err.to_string())
}

pub fn metric_map(metrics: BTreeMap<&'static str, Option<MetricValue>>) -> Value {
    let mut map = Map::new();
    for (name, value) in metrics {
        map.insert(name.to_string(), json!(value));
    }
    Value::Object(map)
}

pub fn artifacts(game_output_path: Option<String>, replay_format: Option<&str>) -> Value {
    let mut value = json!({
        "committed_artifacts": 0,
        "game_output_path": game_output_path,
    });
    if let Some(format) = replay_format {
        value["replay_format"] = json!(format);
    }
    value
}

pub fn confidence_stage0() -> Value {
    bench_contract::confidence_value(1, false)
}

pub fn append_evaluation_fields(report: &mut Value, evaluation: &Evaluation) {
    report["status"] = json!(evaluation.status);
    report["targets"] = bench_contract::targets_value(&evaluation.data.targets);
    report["target_results"] =
        bench_contract::target_results_value(&evaluation.data.target_results);
    report["failures"] = bench_contract::failures_value(&evaluation.data.failures);
    report["confidence"] = confidence_stage0();
}
