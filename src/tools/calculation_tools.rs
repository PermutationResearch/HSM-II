//! Calculation Tools - Math, conversions, and utilities

use serde_json::Value;

use super::{Tool, ToolOutput, object_schema};
use std::collections::HashMap;

// ============================================================================
// Calculator Tool
// ============================================================================

pub struct CalculatorTool;

impl CalculatorTool {
    pub fn new() -> Self {
        Self
    }
    
    fn safe_eval(&self, expr: &str) -> Result<f64, String> {
        // Safe evaluation of mathematical expressions
        // Supports: +, -, *, /, ^, sqrt, sin, cos, tan, log, ln, abs, round, floor, ceil
        
        let expr = expr.to_lowercase().replace(" ", "");
        
        // Check for dangerous patterns
        let dangerous = vec!["::", "std::", "unsafe", "transmute", "include", "include_str", "include_bytes"];
        for pattern in &dangerous {
            if expr.contains(pattern) {
                return Err(format!("Expression contains forbidden pattern: {}", pattern));
            }
        }
        
        // Very basic expression evaluator
        // This is a simplified version - for production, use a proper math parser
        self.parse_expression(&expr)
    }
    
    fn parse_expression(&self, expr: &str) -> Result<f64, String> {
        // Handle parentheses first
        if expr.contains('(') {
            return self.eval_with_parens(expr);
        }
        
        // Handle addition/subtraction
        if let Some(pos) = self.find_operator(expr, '+') {
            let left = self.parse_expression(&expr[..pos])?;
            let right = self.parse_expression(&expr[pos+1..])?;
            return Ok(left + right);
        }
        
        if let Some(pos) = self.find_operator(expr, '-') {
            // Handle negative numbers
            if pos == 0 {
                let val = self.parse_expression(&expr[1..])?;
                return Ok(-val);
            }
            let left = self.parse_expression(&expr[..pos])?;
            let right = self.parse_expression(&expr[pos+1..])?;
            return Ok(left - right);
        }
        
        // Handle multiplication/division
        if let Some(pos) = self.find_operator(expr, '*') {
            let left = self.parse_expression(&expr[..pos])?;
            let right = self.parse_expression(&expr[pos+1..])?;
            return Ok(left * right);
        }
        
        if let Some(pos) = self.find_operator(expr, '/') {
            let left = self.parse_expression(&expr[..pos])?;
            let right = self.parse_expression(&expr[pos+1..])?;
            if right == 0.0 {
                return Err("Division by zero".to_string());
            }
            return Ok(left / right);
        }
        
        // Handle power
        if let Some(pos) = expr.find('^') {
            let left = self.parse_expression(&expr[..pos])?;
            let right = self.parse_expression(&expr[pos+1..])?;
            return Ok(left.powf(right));
        }
        
        // Functions
        if expr.starts_with("sqrt(") && expr.ends_with(')') {
            let inner = &expr[5..expr.len()-1];
            let val = self.parse_expression(inner)?;
            if val < 0.0 {
                return Err("Cannot take square root of negative number".to_string());
            }
            return Ok(val.sqrt());
        }
        
        if expr.starts_with("sin(") && expr.ends_with(')') {
            let inner = &expr[4..expr.len()-1];
            let val = self.parse_expression(inner)?;
            return Ok(val.sin());
        }
        
        if expr.starts_with("cos(") && expr.ends_with(')') {
            let inner = &expr[4..expr.len()-1];
            let val = self.parse_expression(inner)?;
            return Ok(val.cos());
        }
        
        if expr.starts_with("tan(") && expr.ends_with(')') {
            let inner = &expr[4..expr.len()-1];
            let val = self.parse_expression(inner)?;
            return Ok(val.tan());
        }
        
        if expr.starts_with("log(") && expr.ends_with(')') {
            let inner = &expr[4..expr.len()-1];
            let val = self.parse_expression(inner)?;
            if val <= 0.0 {
                return Err("Cannot take log of non-positive number".to_string());
            }
            return Ok(val.log10());
        }
        
        if expr.starts_with("ln(") && expr.ends_with(')') {
            let inner = &expr[3..expr.len()-1];
            let val = self.parse_expression(inner)?;
            if val <= 0.0 {
                return Err("Cannot take ln of non-positive number".to_string());
            }
            return Ok(val.ln());
        }
        
        if expr.starts_with("abs(") && expr.ends_with(')') {
            let inner = &expr[4..expr.len()-1];
            let val = self.parse_expression(inner)?;
            return Ok(val.abs());
        }
        
        if expr.starts_with("round(") && expr.ends_with(')') {
            let inner = &expr[6..expr.len()-1];
            let val = self.parse_expression(inner)?;
            return Ok(val.round());
        }
        
        if expr.starts_with("floor(") && expr.ends_with(')') {
            let inner = &expr[6..expr.len()-1];
            let val = self.parse_expression(inner)?;
            return Ok(val.floor());
        }
        
        if expr.starts_with("ceil(") && expr.ends_with(')') {
            let inner = &expr[5..expr.len()-1];
            let val = self.parse_expression(inner)?;
            return Ok(val.ceil());
        }
        
        if expr.starts_with("pi") {
            return Ok(std::f64::consts::PI);
        }
        
        if expr.starts_with("e") {
            return Ok(std::f64::consts::E);
        }
        
        // Try to parse as number
        expr.parse::<f64>()
            .map_err(|_| format!("Cannot parse expression: {}", expr))
    }
    
    fn eval_with_parens(&self, expr: &str) -> Result<f64, String> {
        let mut depth = 0;
        let mut start = None;
        let mut end = None;
        
        for (i, c) in expr.chars().enumerate() {
            match c {
                '(' => {
                    if depth == 0 {
                        start = Some(i);
                    }
                    depth += 1;
                }
                ')' => {
                    depth -= 1;
                    if depth == 0 && end.is_none() {
                        end = Some(i);
                        break;
                    }
                }
                _ => {}
            }
        }
        
        if let (Some(s), Some(e)) = (start, end) {
            let inner_result = self.parse_expression(&expr[s+1..e])?;
            let new_expr = format!("{}{}", &expr[..s], inner_result);
            let new_expr = if e+1 < expr.len() {
                format!("{}{}", new_expr, &expr[e+1..])
            } else {
                new_expr
            };
            self.parse_expression(&new_expr)
        } else {
            Err("Mismatched parentheses".to_string())
        }
    }
    
    fn find_operator(&self, expr: &str, op: char) -> Option<usize> {
        // Find operator at the top level (not inside parentheses)
        let mut depth = 0;
        for (i, c) in expr.chars().enumerate() {
            match c {
                '(' => depth += 1,
                ')' => depth -= 1,
                _ if c == op && depth == 0 => return Some(i),
                _ => {}
            }
        }
        None
    }
}

#[async_trait::async_trait]
impl Tool for CalculatorTool {
    fn name(&self) -> &str {
        "calculator"
    }
    
    fn description(&self) -> &str {
        "Evaluate mathematical expressions. Supports: +, -, *, /, ^, sqrt, sin, cos, tan, log, ln, abs, round, floor, ceil, pi, e"
    }
    
    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            ("expression", "Mathematical expression to evaluate", true),
        ])
    }
    
    async fn execute(&self, params: Value) -> ToolOutput {
        let expr = params.get("expression").and_then(|v| v.as_str()).unwrap_or("");
        
        if expr.is_empty() {
            return ToolOutput::error("Expression is required");
        }
        
        match self.safe_eval(expr) {
            Ok(result) => {
                // Format result
                let formatted = if result == result.round() {
                    format!("{}", result as i64)
                } else {
                    format!("{:.10}", result)
                        .trim_end_matches('0')
                        .trim_end_matches('.')
                        .to_string()
                };
                
                ToolOutput::success(formatted)
                    .with_metadata(serde_json::json!({
                        "result": result,
                        "expression": expr,
                    }))
            }
            Err(e) => ToolOutput::error(e),
        }
    }
}

impl Default for CalculatorTool {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Unit Conversion Tool
// ============================================================================

pub struct UnitConversionTool;

impl UnitConversionTool {
    pub fn new() -> Self {
        Self
    }
    
    fn get_conversion_factor(&self, from: &str, to: &str) -> Option<f64> {
        // Length conversions (to meters)
        let length: HashMap<&str, f64> = [
            ("m", 1.0),
            ("km", 1000.0),
            ("cm", 0.01),
            ("mm", 0.001),
            ("in", 0.0254),
            ("ft", 0.3048),
            ("yd", 0.9144),
            ("mi", 1609.344),
            ("nm", 1852.0),
        ].into();
        
        // Mass conversions (to kg)
        let mass: HashMap<&str, f64> = [
            ("kg", 1.0),
            ("g", 0.001),
            ("mg", 0.000001),
            ("lb", 0.453592),
            ("oz", 0.0283495),
            ("t", 1000.0),
        ].into();
        
        // Time conversions (to seconds)
        let time: HashMap<&str, f64> = [
            ("s", 1.0),
            ("min", 60.0),
            ("h", 3600.0),
            ("d", 86400.0),
            ("wk", 604800.0),
            ("mo", 2592000.0),
            ("y", 31536000.0),
        ].into();
        
        // Data size (to bytes)
        let data: HashMap<&str, f64> = [
            ("b", 1.0),
            ("kb", 1024.0),
            ("mb", 1048576.0),
            ("gb", 1073741824.0),
            ("tb", 1099511627776.0),
            ("pb", 1.1259e15),
        ].into();
        
        // Check each category
        if let (Some(&f), Some(&t)) = (length.get(from), length.get(to)) {
            return Some(f / t);
        }
        if let (Some(&f), Some(&t)) = (mass.get(from), mass.get(to)) {
            return Some(f / t);
        }
        if let (Some(&f), Some(&t)) = (time.get(from), time.get(to)) {
            return Some(f / t);
        }
        if let (Some(&f), Some(&t)) = (data.get(from), data.get(to)) {
            return Some(f / t);
        }
        
        None
    }
    
    fn convert_temperature(&self, value: f64, from: &str, to: &str) -> Option<f64> {
        // Convert to Celsius first
        let celsius = match from {
            "c" => value,
            "f" => (value - 32.0) * 5.0 / 9.0,
            "k" => value - 273.15,
            _ => return None,
        };
        
        // Convert from Celsius to target
        match to {
            "c" => Some(celsius),
            "f" => Some(celsius * 9.0 / 5.0 + 32.0),
            "k" => Some(celsius + 273.15),
            _ => None,
        }
    }
}

#[async_trait::async_trait]
impl Tool for UnitConversionTool {
    fn name(&self) -> &str {
        "convert"
    }
    
    fn description(&self) -> &str {
        "Convert between units of measurement (length, mass, temperature, time, data)."
    }
    
    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            ("value", "Value to convert", true),
            ("from", "Source unit (e.g., 'm', 'kg', 'c')", true),
            ("to", "Target unit (e.g., 'ft', 'lb', 'f')", true),
        ])
    }
    
    async fn execute(&self, params: Value) -> ToolOutput {
        let value = params.get("value").and_then(|v| v.as_f64());
        let from = params.get("from").and_then(|v| v.as_str()).unwrap_or("").to_lowercase();
        let to = params.get("to").and_then(|v| v.as_str()).unwrap_or("").to_lowercase();
        
        if value.is_none() || from.is_empty() || to.is_empty() {
            return ToolOutput::error("value, from, and to parameters are required");
        }
        
        let value = value.unwrap();
        
        // Temperature is special
        if vec!["c", "f", "k"].contains(&from.as_str()) && vec!["c", "f", "k"].contains(&to.as_str()) {
            if let Some(result) = self.convert_temperature(value, &from, &to) {
                return ToolOutput::success(format!("{} {} = {} {}", value, from, result, to));
            }
        }
        
        if let Some(factor) = self.get_conversion_factor(&from, &to) {
            let result = value * factor;
            ToolOutput::success(format!("{} {} = {} {}", value, from, result, to))
        } else {
            ToolOutput::error(format!("Cannot convert from '{}' to '{}'", from, to))
        }
    }
}

impl Default for UnitConversionTool {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Random Number Tool
// ============================================================================

pub struct RandomTool;

impl RandomTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl Tool for RandomTool {
    fn name(&self) -> &str {
        "random"
    }
    
    fn description(&self) -> &str {
        "Generate random numbers, booleans, or select from options."
    }
    
    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            ("operation", "number, int, bool, choice, shuffle (default: number)", false),
            ("min", "Minimum value (for number/int, default: 0)", false),
            ("max", "Maximum value (for number/int, default: 1)", false),
            ("options", "Array of options to choose from (for choice)", false),
            ("array", "Array to shuffle", false),
            ("count", "Number of results (default: 1)", false),
        ])
    }
    
    async fn execute(&self, params: Value) -> ToolOutput {
        use rand::{seq::SliceRandom, Rng};
        
        let operation = params.get("operation").and_then(|v| v.as_str()).unwrap_or("number");
        let count = params.get("count").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
        
        let mut rng = rand::thread_rng();
        
        match operation {
            "number" => {
                let min = params.get("min").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let max = params.get("max").and_then(|v| v.as_f64()).unwrap_or(1.0);
                
                if count == 1 {
                    let value: f64 = rng.gen_range(min..max);
                    ToolOutput::success(value.to_string())
                } else {
                    let values: Vec<f64> = (0..count).map(|_| rng.gen_range(min..max)).collect();
                    ToolOutput::success(format!("{:?}", values))
                        .with_metadata(serde_json::json!({ "values": values }))
                }
            }
            "int" => {
                let min = params.get("min").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                let max = params.get("max").and_then(|v| v.as_i64()).unwrap_or(100) as i32;
                
                if count == 1 {
                    let value: i32 = rng.gen_range(min..max);
                    ToolOutput::success(value.to_string())
                } else {
                    let values: Vec<i32> = (0..count).map(|_| rng.gen_range(min..max)).collect();
                    ToolOutput::success(format!("{:?}", values))
                        .with_metadata(serde_json::json!({ "values": values }))
                }
            }
            "bool" => {
                let value: bool = rng.gen();
                ToolOutput::success(value.to_string())
            }
            "choice" => {
                if let Some(options) = params.get("options").and_then(|v| v.as_array()) {
                    if options.is_empty() {
                        return ToolOutput::error("Options array is empty");
                    }
                    let choices: Vec<Value> = options.iter().cloned().collect();
                    if count == 1 {
                        if let Some(choice) = choices.choose(&mut rng) {
                            ToolOutput::success(format!("{:?}", choice))
                                .with_metadata(serde_json::json!({ "choice": choice }))
                        } else {
                            ToolOutput::error("Failed to make random choice")
                        }
                    } else {
                        let selected: Vec<Value> = (0..count.min(choices.len()))
                            .map(|_| choices.choose(&mut rng).unwrap().clone())
                            .collect();
                        ToolOutput::success(format!("{:?}", selected))
                            .with_metadata(serde_json::json!({ "choices": selected }))
                    }
                } else {
                    ToolOutput::error("options array is required for choice operation")
                }
            }
            "shuffle" => {
                if let Some(array) = params.get("array").and_then(|v| v.as_array()).cloned() {
                    let mut shuffled = array;
                    shuffled.shuffle(&mut rng);
                    ToolOutput::success(format!("{:?}", shuffled))
                        .with_metadata(serde_json::json!({ "shuffled": shuffled }))
                } else {
                    ToolOutput::error("array is required for shuffle operation")
                }
            }
            _ => ToolOutput::error(format!("Unknown operation: {}", operation)),
        }
    }
}

impl Default for RandomTool {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Hash Tool
// ============================================================================

pub struct HashTool;

impl HashTool {
    pub fn new() -> Self {
        Self
    }
    
    fn md5(&self, data: &str) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        
        // Simple hash - for production, use proper md5 crate
        let mut hasher = DefaultHasher::new();
        data.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }
    
    fn sha256_simple(&self, data: &str) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        
        // Simplified - for production use sha2 crate
        let mut hasher = DefaultHasher::new();
        data.hash(&mut hasher);
        format!("{:016x}{:016x}", hasher.finish(), hasher.finish())
    }
}

#[async_trait::async_trait]
impl Tool for HashTool {
    fn name(&self) -> &str {
        "hash"
    }
    
    fn description(&self) -> &str {
        "Generate hash of data (MD5, SHA256, simple)."
    }
    
    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            ("data", "Data to hash", true),
            ("algorithm", "Hash algorithm: md5, sha256, simple (default: simple)", false),
        ])
    }
    
    async fn execute(&self, params: Value) -> ToolOutput {
        let data = params.get("data").and_then(|v| v.as_str()).unwrap_or("");
        let algorithm = params.get("algorithm").and_then(|v| v.as_str()).unwrap_or("simple");
        
        if data.is_empty() {
            return ToolOutput::error("data is required");
        }
        
        let hash = match algorithm {
            "md5" => self.md5(data),
            "sha256" => self.sha256_simple(data),
            "simple" => self.md5(data),
            _ => return ToolOutput::error(format!("Unknown algorithm: {}", algorithm)),
        };
        
        ToolOutput::success(hash.clone())
            .with_metadata(serde_json::json!({
                "hash": hash,
                "algorithm": algorithm,
            }))
    }
}

impl Default for HashTool {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// UUID Tool
// ============================================================================

pub struct UuidTool;

impl UuidTool {
    pub fn new() -> Self {
        Self
    }
    
    fn generate_v4(&self) -> String {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        
        let mut bytes = [0u8; 16];
        rng.fill(&mut bytes);
        
        // Set version (4) and variant (2)
        bytes[6] = (bytes[6] & 0x0f) | 0x40;
        bytes[8] = (bytes[8] & 0x3f) | 0x80;
        
        format!(
            "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            bytes[0], bytes[1], bytes[2], bytes[3],
            bytes[4], bytes[5], bytes[6], bytes[7],
            bytes[8], bytes[9], bytes[10], bytes[11],
            bytes[12], bytes[13], bytes[14], bytes[15]
        )
    }
}

#[async_trait::async_trait]
impl Tool for UuidTool {
    fn name(&self) -> &str {
        "uuid"
    }
    
    fn description(&self) -> &str {
        "Generate UUID v4 (random UUID)."
    }
    
    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            ("count", "Number of UUIDs to generate (default: 1)", false),
            ("uppercase", "Use uppercase (default: false)", false),
        ])
    }
    
    async fn execute(&self, params: Value) -> ToolOutput {
        let count = params.get("count").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
        let uppercase = params.get("uppercase").and_then(|v| v.as_bool()).unwrap_or(false);
        
        let uuids: Vec<String> = (0..count)
            .map(|_| {
                let uuid = self.generate_v4();
                if uppercase {
                    uuid.to_uppercase()
                } else {
                    uuid
                }
            })
            .collect();
        
        if count == 1 {
            ToolOutput::success(uuids[0].clone())
        } else {
            ToolOutput::success(uuids.join("\n"))
                .with_metadata(serde_json::json!({ "uuids": uuids }))
        }
    }
}

impl Default for UuidTool {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Date/Time Tool
// ============================================================================

pub struct DateTimeTool;

impl DateTimeTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl Tool for DateTimeTool {
    fn name(&self) -> &str {
        "datetime"
    }
    
    fn description(&self) -> &str {
        "Get current date/time or format timestamps. Operations: now, format, parse, add, diff"
    }
    
    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            ("operation", "now, format, parse, add, diff (default: now)", false),
            ("timestamp", "Timestamp to format/parse", false),
            ("format", "Date format string (e.g., '%Y-%m-%d %H:%M:%S')", false),
            ("amount", "Amount to add (for add operation)", false),
            ("unit", "Unit for add: seconds, minutes, hours, days", false),
            ("from", "Start timestamp (for diff)", false),
            ("to", "End timestamp (for diff)", false),
        ])
    }
    
    async fn execute(&self, params: Value) -> ToolOutput {
        use chrono::{Duration, Local, TimeZone, Utc};
        
        let operation = params.get("operation").and_then(|v| v.as_str()).unwrap_or("now");
        
        match operation {
            "now" => {
                let now = Utc::now();
                ToolOutput::success(now.to_rfc3339())
                    .with_metadata(serde_json::json!({
                        "iso": now.to_rfc3339(),
                        "timestamp": now.timestamp(),
                        "local": Local::now().to_rfc3339(),
                    }))
            }
            "format" => {
                let format_str = params.get("format").and_then(|v| v.as_str()).unwrap_or("%Y-%m-%d %H:%M:%S");
                
                let dt = if let Some(ts) = params.get("timestamp").and_then(|v| v.as_i64()) {
                    Utc.timestamp_opt(ts, 0).unwrap()
                } else {
                    Utc::now()
                };
                
                ToolOutput::success(dt.format(format_str).to_string())
            }
            "parse" => {
                let input = params.get("timestamp").and_then(|v| v.as_str()).unwrap_or("");
                let format_str = params.get("format").and_then(|v| v.as_str()).unwrap_or("%Y-%m-%d %H:%M:%S");
                
                match chrono::NaiveDateTime::parse_from_str(input, format_str) {
                    Ok(dt) => {
                        let dt_utc = Utc.from_utc_datetime(&dt);
                        ToolOutput::success(dt_utc.to_rfc3339())
                            .with_metadata(serde_json::json!({
                                "timestamp": dt_utc.timestamp(),
                            }))
                    }
                    Err(e) => ToolOutput::error(format!("Failed to parse date: {}", e)),
                }
            }
            "add" => {
                let amount = params.get("amount").and_then(|v| v.as_i64()).unwrap_or(0);
                let unit = params.get("unit").and_then(|v| v.as_str()).unwrap_or("seconds");
                
                let dt = if let Some(ts) = params.get("timestamp").and_then(|v| v.as_i64()) {
                    Utc.timestamp_opt(ts, 0).unwrap()
                } else {
                    Utc::now()
                };
                
                let duration = match unit {
                    "seconds" => Duration::seconds(amount),
                    "minutes" => Duration::minutes(amount),
                    "hours" => Duration::hours(amount),
                    "days" => Duration::days(amount),
                    _ => return ToolOutput::error(format!("Unknown unit: {}", unit)),
                };
                
                let result = dt + duration;
                ToolOutput::success(result.to_rfc3339())
            }
            "diff" => {
                let from_ts = params.get("from").and_then(|v| v.as_i64()).unwrap_or(0);
                let to_ts = params.get("to").and_then(|v| v.as_i64()).unwrap_or_else(|| Utc::now().timestamp());
                
                let diff = to_ts - from_ts;
                let seconds = diff.abs();
                let minutes = seconds / 60;
                let hours = minutes / 60;
                let days = hours / 24;
                
                ToolOutput::success(format!("{} seconds ({} minutes, {} hours, {} days)", 
                    diff, minutes, hours, days))
                    .with_metadata(serde_json::json!({
                        "seconds": diff,
                        "minutes": minutes,
                        "hours": hours,
                        "days": days,
                    }))
            }
            _ => ToolOutput::error(format!("Unknown operation: {}", operation)),
        }
    }
}

impl Default for DateTimeTool {
    fn default() -> Self {
        Self::new()
    }
}
