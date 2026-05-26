use crate::parser::ast::*;
use crate::runtime::value::CoroutineInstr;

pub(super) fn compile_coroutine_body(statements: &[Stmt]) -> Vec<CoroutineInstr> {
    let mut instructions = Vec::new();
    let mut loop_stack = Vec::new();
    compile_statements(statements, &mut instructions, &mut loop_stack);
    instructions
}

struct LoopTargets {
    continue_target: usize,
    break_jumps: Vec<usize>,
}

fn compile_statements(
    statements: &[Stmt],
    instructions: &mut Vec<CoroutineInstr>,
    loop_stack: &mut Vec<LoopTargets>,
) {
    for stmt in statements {
        compile_statement(stmt, instructions, loop_stack);
    }
}

fn compile_statement(
    stmt: &Stmt,
    instructions: &mut Vec<CoroutineInstr>,
    loop_stack: &mut Vec<LoopTargets>,
) {
    match stmt {
        Stmt::Yield { value, .. } => instructions.push(CoroutineInstr::Yield(value.clone())),
        Stmt::TryCatch { .. } => instructions.push(CoroutineInstr::Stmt(stmt.clone())),
        Stmt::While { condition, body } => {
            let start = instructions.len();
            let jump_if_false = instructions.len();
            instructions.push(CoroutineInstr::JumpIfFalse {
                condition: condition.clone(),
                target: usize::MAX,
            });
            loop_stack.push(LoopTargets {
                continue_target: start,
                break_jumps: Vec::new(),
            });
            compile_statements(body, instructions, loop_stack);
            let loop_targets = loop_stack.pop().expect("loop target stack underflow");
            instructions.push(CoroutineInstr::Jump { target: start });
            let end = instructions.len();
            patch_jump_target(instructions, jump_if_false, end);
            for break_jump in loop_targets.break_jumps {
                patch_jump_target(instructions, break_jump, end);
            }
        }
        Stmt::If {
            condition,
            then_branch,
            elifs,
            else_branch,
        } => {
            let mut end_jumps = Vec::new();
            let first_false_jump = instructions.len();
            instructions.push(CoroutineInstr::JumpIfFalse {
                condition: condition.clone(),
                target: usize::MAX,
            });
            compile_statements(then_branch, instructions, loop_stack);
            end_jumps.push(push_jump_placeholder(instructions));
            let mut previous_false_jump = first_false_jump;

            for (condition, body) in elifs {
                let elif_start = instructions.len();
                patch_jump_target(instructions, previous_false_jump, elif_start);
                previous_false_jump = instructions.len();
                instructions.push(CoroutineInstr::JumpIfFalse {
                    condition: condition.clone(),
                    target: usize::MAX,
                });
                compile_statements(body, instructions, loop_stack);
                end_jumps.push(push_jump_placeholder(instructions));
            }

            let else_start = instructions.len();
            patch_jump_target(instructions, previous_false_jump, else_start);
            if let Some(else_branch) = else_branch {
                compile_statements(else_branch, instructions, loop_stack);
            }
            let end = instructions.len();
            for jump in end_jumps {
                patch_jump_target(instructions, jump, end);
            }
        }
        Stmt::Break(_) => {
            if let Some(targets) = loop_stack.last_mut() {
                let jump = push_jump_placeholder(instructions);
                targets.break_jumps.push(jump);
            } else {
                instructions.push(CoroutineInstr::Stmt(stmt.clone()));
            }
        }
        Stmt::Continue(_) => {
            if let Some(targets) = loop_stack.last() {
                instructions.push(CoroutineInstr::Jump {
                    target: targets.continue_target,
                });
            } else {
                instructions.push(CoroutineInstr::Stmt(stmt.clone()));
            }
        }
        Stmt::Match { .. } => instructions.push(CoroutineInstr::Stmt(stmt.clone())),
        _ => instructions.push(CoroutineInstr::Stmt(stmt.clone())),
    }
}

fn push_jump_placeholder(instructions: &mut Vec<CoroutineInstr>) -> usize {
    let index = instructions.len();
    instructions.push(CoroutineInstr::Jump { target: usize::MAX });
    index
}

fn patch_jump_target(instructions: &mut [CoroutineInstr], index: usize, target: usize) {
    match &mut instructions[index] {
        CoroutineInstr::JumpIfFalse { target: slot, .. }
        | CoroutineInstr::Jump { target: slot } => {
            *slot = target;
        }
        _ => {}
    }
}
