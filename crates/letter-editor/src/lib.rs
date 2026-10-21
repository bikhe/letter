//! Редактирование: курсор, выделение, команды, отмена и повтор.
//!
//! - [`position`] — позиция курсора в документе;
//! - [`caret`](mod@caret) — где на странице нарисовать курсор и какая позиция под
//!   указателем мыши;
//! - [`navigation`] — перемещение курсора с клавиатуры;
//! - [`edit`] — ввод и удаление текста.

pub mod caret;
pub mod edit;
pub mod navigation;
pub mod position;

pub use caret::{Caret, CaretStop, caret, hit_test, line_stops, position_in_line};
pub use edit::{Insert, delete_backward, delete_forward, insert, split_paragraph};
pub use navigation::{Motion, Moved, move_cursor};
pub use position::{Affinity, Position};
