//! Application screens.

pub mod connections;
pub mod workspace;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Connections,
    Workspace,
}
