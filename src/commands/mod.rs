pub mod clip_commands;
pub mod transform_commands;
pub mod transition_commands;

use anyhow::Result;

use crate::state::AppState;

pub trait Command {
    fn execute(&mut self, state: &mut AppState) -> Result<()>;
    fn undo(&mut self, state: &mut AppState) -> Result<()>;
    #[allow(dead_code)]
    fn description(&self) -> &str;
}

pub struct CommandHistory {
    undo_stack: Vec<Box<dyn Command>>,
    redo_stack: Vec<Box<dyn Command>>,
}

impl CommandHistory {
    pub fn new() -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }

    pub fn execute(&mut self, mut cmd: Box<dyn Command>, state: &mut AppState) -> Result<()> {
        cmd.execute(state)?;
        self.undo_stack.push(cmd);
        self.redo_stack.clear();
        if self.undo_stack.len() > 100 {
            self.undo_stack.remove(0);
        }
        Ok(())
    }

    pub fn undo(&mut self, state: &mut AppState) -> Result<()> {
        if let Some(mut cmd) = self.undo_stack.pop() {
            cmd.undo(state)?;
            self.redo_stack.push(cmd);
        }
        Ok(())
    }

    pub fn redo(&mut self, state: &mut AppState) -> Result<()> {
        if let Some(mut cmd) = self.redo_stack.pop() {
            cmd.execute(state)?;
            self.undo_stack.push(cmd);
        }
        Ok(())
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }
}
