use studio_core::EngineKind;

/// 三种状态标记，供 doctor 输出使用。
pub enum Status {
    Ok,
    Warn,
    Fail,
}

impl Status {
    pub fn as_str(&self) -> &'static str {
        match self {
            Status::Ok => "OK  ",
            Status::Warn => "WARN",
            Status::Fail => "FAIL",
        }
    }
}

pub fn title(text: &str) {
    println!("\n== {text} ==");
}

pub fn kv(key: &str, value: impl std::fmt::Display) {
    println!("  {key:<18} {value}");
}

pub fn status(status: Status, message: impl std::fmt::Display) {
    println!("  [{}] {message}", status.as_str());
}

/// 极简表格：按列宽对齐，不引入额外依赖。
pub fn table(headers: &[&str], rows: &[Vec<String>]) {
    let widths: Vec<usize> = headers
        .iter()
        .enumerate()
        .map(|(idx, header)| {
            rows.iter()
                .map(|row| row.get(idx).map(|c| c.chars().count()).unwrap_or(0))
                .chain(std::iter::once(header.chars().count()))
                .max()
                .unwrap_or(0)
        })
        .collect();

    let line = |cells: &[String]| {
        let mut out = String::new();
        for (idx, cell) in cells.iter().enumerate() {
            if idx > 0 {
                out.push_str("  ");
            }
            out.push_str(cell);
            let pad = widths[idx].saturating_sub(cell.chars().count());
            out.push_str(&" ".repeat(pad));
        }
        out
    };

    let header_cells: Vec<String> = headers.iter().map(|h| h.to_string()).collect();
    println!("  {}", line(&header_cells));
    println!(
        "  {}",
        "-".repeat(widths.iter().sum::<usize>() + 2 * widths.len())
    );
    for row in rows {
        println!("  {}", line(row));
    }
}

/// 把引擎列表格式化成单元格。
pub fn engine_list(engines: &[EngineKind]) -> String {
    engines
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join(",")
}
