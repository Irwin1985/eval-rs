use crate::array::ArraySize;
use crate::bytecode::{ByteCode, Opcode, Register};
use crate::error::{err_eval, RuntimeError};
use crate::memory::MutatorView;
use crate::pair::{get_one_from_pair_list, get_two_from_pair_list};
use crate::safeptr::{ScopedPtr, TaggedScopedPtr};
use crate::taggedptr::Value;

struct Compiler {
    bytecode: ByteCode,
    next_reg: Register,
}

impl Compiler {
    fn new() -> Compiler {
        Compiler {
            bytecode: ByteCode::new(),
            next_reg: 0,
        }
    }

    fn compile<'guard>(
        &mut self,
        mem: &'guard MutatorView,
        ast: TaggedScopedPtr<'guard>,
    ) -> Result<(), RuntimeError> {
        let result_reg = self.compile_eval(mem, ast)?;
        self.bytecode.push_op1(mem, Opcode::RETURN, result_reg)?;
        Ok(())
    }

    fn compile_eval<'guard>(
        &mut self,
        mem: &'guard MutatorView,
        ast_node: TaggedScopedPtr<'guard>,
    ) -> Result<Register, RuntimeError> {
        match *ast_node {
            Value::Pair(p) => self.compile_apply(mem, p.first.get(mem), p.second.get(mem)),

            Value::Symbol(s) => {
                let literal = match s.as_str(mem) {
                    "nil" => mem.nil(),
                    // TODO there will be an environment where symbols will be bound, this shouldn't be
                    // a literal but an environment lookup
                    _ => ast_node,
                };
                self.push_load_literal(mem, literal)
            }

            _ => self.push_load_literal(mem, ast_node),
        }
    }

    fn compile_apply<'guard>(
        &mut self,
        mem: &'guard MutatorView,
        function: TaggedScopedPtr<'guard>,
        params: TaggedScopedPtr<'guard>,
    ) -> Result<Register, RuntimeError> {
        match *function {
            Value::Symbol(s) => match s.as_str(mem) {
                "quote" => self.push_load_literal(mem, get_one_from_pair_list(mem, params)?),
                "atom" => self.push_op2(mem, Opcode::ATOM, params),
                "car" => self.push_op2(mem, Opcode::CAR, params),
                "cdr" => self.push_op2(mem, Opcode::CDR, params),
                "cons" => self.push_op3(mem, Opcode::CONS, params),
                "cond" => self.compile_apply_cond(mem, params),
                "eq" => self.push_op3(mem, Opcode::EQ, params),

                _ => Err(err_eval("Symbol is not bound to a function")),
            },

            _ => Err(err_eval("Non symbol in function-call position")),
        }
    }

    fn compile_apply_cond<'guard>(
        &mut self,
        mem: &'guard MutatorView,
        params: TaggedScopedPtr<'guard>,
    ) -> Result<Register, RuntimeError> {
        //
        //   for each param:
        //     eval cond
        //     if false then jmp -> next
        //     else eval expr
        //     jmp -> end
        //
        let mut end_jumps: Vec<ArraySize> = Vec::new();
        let mut last_cond_jump: Option<ArraySize> = None;

        let result = self.next_reg;

        let mut head = params;
        while let Value::Pair(p) = *head {
            let cond = p.first.get(mem);
            head = p.second.get(mem);
            match *head {
                Value::Pair(p) => {
                    let expr = p.first.get(mem);
                    head = p.second.get(mem);

                    // if this is not the first condition, set the offset of the last
                    // condition-not-true jump to the beginning of this condition
                    if let Some(address) = last_cond_jump {
                        let offset = self.bytecode.next_instruction() - address - 1;
                        self.bytecode.write_jump_offset(mem, address, offset)?;
                    }

                    // We have a condition to evaluate. If the resut is Not True, jump to the
                    // next condition.
                    self.reset_reg(result); // reuse this register for condition and result
                    let cond_result = self.compile_eval(mem, cond)?;
                    self.bytecode
                        .push_cond_jump(mem, Opcode::JMPNT, cond_result)?;
                    last_cond_jump = Some(self.bytecode.last_instruction());

                    // Compile the expression and jump to the end of the entire cond
                    self.reset_reg(result); // reuse this register for condition and result
                    let _expr_result = self.compile_eval(mem, expr)?;
                    self.bytecode.push_jump(mem)?;
                    end_jumps.push(self.bytecode.last_instruction());
                }

                _ => return Err(err_eval("Unexpected end of cond list")),
            }
        }

        // Close out with a default NIL result if none of the conditions passed
        if let Some(address) = last_cond_jump {
            self.reset_reg(result);
            self.push_op1(mem, Opcode::LOADNIL)?;
            let offset = self.bytecode.next_instruction() - address - 1;
            self.bytecode.write_jump_offset(mem, address, offset)?;
        }

        // Update all the post-expr jumps to point at the next instruction after the entire cond
        for address in end_jumps.iter() {
            let offset = self.bytecode.next_instruction() - address - 1;
            self.bytecode.write_jump_offset(mem, *address, offset)?;
        }

        Ok(result)
    }

    fn push_op0<'guard>(
        &mut self,
        mem: &'guard MutatorView,
        op: Opcode,
    ) -> Result<(), RuntimeError> {
        self.bytecode.push_op0(mem, op)?;
        Ok(())
    }

    fn push_op1<'guard>(
        &mut self,
        mem: &'guard MutatorView,
        op: Opcode,
    ) -> Result<Register, RuntimeError> {
        let result = self.acquire_reg();
        self.bytecode.push_op1(mem, op, result)?;
        Ok(result)
    }

    fn push_op2<'guard>(
        &mut self,
        mem: &'guard MutatorView,
        op: Opcode,
        params: TaggedScopedPtr<'guard>,
    ) -> Result<Register, RuntimeError> {
        let result = self.acquire_reg();
        let reg1 = self.compile_eval(mem, get_one_from_pair_list(mem, params)?)?;
        self.bytecode.push_op2(mem, op, result, reg1)?;
        Ok(result)
    }

    fn push_op3<'guard>(
        &mut self,
        mem: &'guard MutatorView,
        op: Opcode,
        params: TaggedScopedPtr<'guard>,
    ) -> Result<Register, RuntimeError> {
        let result = self.acquire_reg();
        let (first, second) = get_two_from_pair_list(mem, params)?;
        let reg1 = self.compile_eval(mem, first)?;
        let reg2 = self.compile_eval(mem, second)?;
        self.bytecode.push_op3(mem, op, result, reg1, reg2)?;
        Ok(result)
    }

    // Push a literal onto the literals list and a load instruction onto the bytecode list
    fn push_load_literal<'guard>(
        &mut self,
        mem: &'guard MutatorView,
        literal: TaggedScopedPtr<'guard>,
    ) -> Result<Register, RuntimeError> {
        let reg = self.acquire_reg();
        let lit_id = self.bytecode.push_lit(mem, literal)?;
        self.bytecode.push_loadlit(mem, reg, lit_id)?;
        Ok(reg)
    }

    // this is a naive way of allocating registers - every result gets it's own register
    fn acquire_reg(&mut self) -> Register {
        let reg = self.next_reg;
        self.next_reg += 1;
        reg
    }

    // reset the next register back to the given one so that it is reused
    fn reset_reg(&mut self, reg: Register) {
        self.next_reg = reg
    }
}

/// Compile the given AST and return a bytecode structure
pub fn compile<'guard>(
    mem: &'guard MutatorView,
    ast: TaggedScopedPtr<'guard>,
) -> Result<ScopedPtr<'guard, ByteCode>, RuntimeError> {
    let mut compiler = Compiler::new();
    compiler.compile(mem, ast)?;

    let bytecode = compiler.bytecode;
    mem.alloc(bytecode)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn compile_cond() {
        // TODO
    }
}
