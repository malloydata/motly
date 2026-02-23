use motly_rust::{parse_motly, tree::MOTLYNode};

use std::io::{self, Read};

fn main() {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input).unwrap();

    let result = parse_motly(&input, MOTLYNode::new());
    if result.errors.is_empty() {
        println!("{}", result.value.to_json_pretty());
        return;
    }

    let lines: Vec<&str> = input.lines().collect();

    for err in &result.errors {
        let line_num = err.begin.line;
        let line_text = lines.get(line_num).unwrap_or(&"");

        eprintln!("ERROR AT LINE {}:", line_num + 1);
        eprintln!("{}", line_text);

        // Build the underline
        let start_col = err.begin.column;
        let end_col = if err.begin.line == err.end.line && err.end.column > err.begin.column {
            err.end.column
        } else {
            // Point error or spans multiple lines: underline to end of line
            if start_col < line_text.len() {
                line_text.len()
            } else {
                start_col + 1
            }
        };

        let mut underline = String::new();
        for _ in 0..start_col {
            underline.push(' ');
        }
        underline.push('^');
        if end_col > start_col + 1 {
            for _ in (start_col + 1)..end_col {
                underline.push('_');
            }
        }

        eprintln!("{}", underline);
        eprintln!("{}", err.message);
        eprintln!();
    }

    std::process::exit(1);
}
