//! Startup banner.

use std::io::IsTerminal;

/// 启动 LOGO（figlet nancyj 字体渲染的 "OwO"）
pub const LOGO: &str = indoc::indoc! {r#"
                                                       
    ,ad8888ba,                          ,ad8888ba,    
   d8"'    `"8b                        d8"'    `"8b   
  d8'        `8b                      d8'        `8b  
  88          88  8b      db      d8  88          88  
  88          88  `8b    d88b    d8'  88          88  
  Y8,        ,8P   `8b  d8'`8b  d8'   Y8,        ,8P  
   Y8a.    .a8P     `8bd8'  `8bd8'     Y8a.    .a8P   
    `"Y8888Y"'        YP      YP        `"Y8888Y"'    
                                                      
"#};

/// Banner 副标题（项目名）
pub const TITLE: &str = "OwO AI Gateway";
/// 版本号，由 Cargo.toml 在编译期写入
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Prints the logo with the title centred under it when stdout is a terminal; otherwise
/// a single `OwO AI Gateway <version>` line, so piped and logged output stays plain.
pub fn print() {
    if !std::io::stdout().is_terminal() {
        println!("{TITLE} {VERSION}");
        return;
    }
    let width = LOGO.lines().map(str::len).max().unwrap_or(0);
    for line in LOGO.lines() {
        println!("  {}", line.trim_end());
    }
    println!("  {:^width$}\n", format!("{TITLE}  v{VERSION}"));
}
