mod conversations;
mod cursor;
mod ingestion;
mod instruction;
mod io;
mod quota;
mod settings;
mod usage;

pub use conversations::*;
pub use cursor::*;
pub use ingestion::*;
pub use instruction::*;
pub use io::*;
pub use quota::*;
pub use settings::*;
pub use usage::*;

#[tauri::command]
pub fn ping() -> String {
    "pong".to_string()
}
