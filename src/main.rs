#![feature(proc_macro_hygiene)]

use dynasm::dynasm;
use dynasmrt::{DynasmApi, DynamicLabel, DynasmLabelApi};

use std::env;
use std::fs;

use std::mem;
use std::collections::HashMap;
use std::process::exit;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 3 {
        println!("Usage: {} <files-with-intcode> <input>", args[0]);
        exit(1);
    }
    let code = fs::read_to_string(&args[1]).ok().unwrap();
    let input: i64 = args[2].parse().unwrap();
    let result = compile(&code)(input);
    println!("Result: {}", result);
}

#[derive(PartialEq)]
enum ParameterMode {
    Position,
    Immediate,
}

fn translate(
    mem: &[i64],
    offset: &mut usize,
    ops: &mut dynasmrt::Assembler<dynasmrt::x64::X64Relocation>,
    labels: &mut HashMap<usize, DynamicLabel>,
) {
    let inst = mem[*offset];
    let opcode = inst % 100;
    let mode1 = match inst / 100 % 10 {
        1 => ParameterMode::Immediate,
        _ => ParameterMode::Position
    };
    let mode2 = match inst / 1_000 % 10 {
        1 => ParameterMode::Immediate,
        _ => ParameterMode::Position
    };
    let label = labels.entry(*offset).or_insert_with(|| ops.new_dynamic_label());
    ops.dynamic_label(*label);
    match opcode {
        1 => { // add src1, src2, dst
            match mode1 {
                ParameterMode::Position => {
                    dynasm!(ops
                        ; mov r8, QWORD [rdi + mem[*offset + 1] as i32 * 8]
                    );
                }
                ParameterMode::Immediate => {
                    dynasm!(ops
                        ; mov r8, QWORD mem[*offset + 1]
                    );
                }
            }
            match mode2 {
                ParameterMode::Position => {
                    dynasm!(ops
                        ; add r8, QWORD [rdi + mem[*offset + 2] as i32 * 8]
                    );
                }
                ParameterMode::Immediate => {
                    dynasm!(ops
                        ; mov r9, QWORD mem[*offset + 2]
                        ; add r8, r9
                    );
                }
            }
            dynasm!(ops
                ; mov QWORD [rdi + mem[*offset + 3] as i32 * 8], r8
            );
            *offset += 4;
        }
        2 => { // mul src1, src2, dst
            match mode1 {
                ParameterMode::Position => {
                    dynasm!(ops
                        ; mov r8, QWORD [rdi + mem[*offset + 1] as i32 * 8]
                    );
                }
                ParameterMode::Immediate => {
                    dynasm!(ops
                        ; mov r8, QWORD mem[*offset + 1]
                    );
                }
            }
            match mode2 {
                ParameterMode::Position => {
                    dynasm!(ops
                        ; imul r8, QWORD [rdi + mem[*offset + 2] as i32 * 8]
                    );
                }
                ParameterMode::Immediate => {
                    dynasm!(ops
                        ; mov r9, QWORD mem[*offset + 2]
                        ; imul r8, r9
                    );
                }
            }
            dynasm!(ops
                ; mov QWORD [rdi + mem[*offset + 3] as i32 * 8], r8
            );
            *offset += 4;
        }
        3 => {
            // input src
            dynasm!(ops
                ; mov QWORD [rdi + mem[*offset + 1] as i32 * 8], rsi
            );
            *offset += 2;
        }
        4 => { // output dst
            match mode1 {
                ParameterMode::Position => {
                    dynasm!(ops
                        ; mov rax, QWORD [rdi + mem[*offset + 1] as i32 * 8]
                    );
                }
                ParameterMode::Immediate => {
                    dynasm!(ops
                        ; mov rax, QWORD mem[*offset + 1]
                    );
                }
            }
            *offset += 2;
        }
        5 | 6 => { // jump_not_zero/jump_zero cond, target
            if mode2 == ParameterMode::Position {
                panic!("indirect jumps are NYI");
            }
            let target = labels.entry(mem[*offset + 2] as usize).or_insert_with(|| ops.new_dynamic_label());
            match mode1 {
                ParameterMode::Position => {
                    dynasm!(ops
                        ; cmp QWORD [rdi + mem[*offset + 1] as i32 * 8], 0
                    );
                    if opcode == 5 {
                        dynasm!(ops
                            ; jnz =>*target
                        );
                    } else {
                        dynasm!(ops
                            ; jz =>*target
                        );
                    }
                }
                ParameterMode::Immediate => {
                    // does not support self-modifying code (which isn't supported anyways due to icache coherency)
                    if (opcode == 5) == (mem[*offset + 1] != 0) {
                        dynasm!(ops
                            ; jmp =>*target
                        );
                    }
                }
            }
            *offset += 3;
        }
        7 | 8 => { // less_than/equals src1, src2, dst
            match mode1 {
                ParameterMode::Position => {
                    dynasm!(ops
                        ; mov r8, QWORD [rdi + mem[*offset + 1] as i32 * 8]
                    );
                }
                ParameterMode::Immediate => {
                    dynasm!(ops
                        ; mov r8, QWORD mem[*offset + 1]
                    );
                }
            }
            match mode2 {
                ParameterMode::Position => {
                    dynasm!(ops
                        ; cmp r8, QWORD [rdi + mem[*offset + 2] as i32 * 8]
                    );
                }
                ParameterMode::Immediate => {
                    dynasm!(ops
                        ; mov r9, QWORD mem[*offset + 2]
                        ; cmp r8, r9
                    );
                }
            }
            dynasm!(ops
                ; mov r8, 0
                ; mov r9, 1
            );
            if opcode == 7 {
                dynasm!(ops
                    ; cmovl r8, r9
                );
            } else {
                dynasm!(ops
                    ; cmovz r8, r9
                );
            }
            dynasm!(ops
                ; mov QWORD [rdi + mem[*offset + 3] as i32 * 8], r8
            );
            *offset += 4;
        }
        99 => {
            // halt
            dynasm!(ops
                ; ret
            );
            *offset += 1;
        }
        _ => {
            // we just skip over unknown opcodes, this allows for "holes" in the program
            *offset += 1;
        }
    }
}

fn compile(code_str: &str) -> impl FnMut(i64) -> i64 {
    let mut memory: Vec<i64> = code_str.split(',').map(|e| e.trim().parse::<i64>().unwrap()).collect();
    let mut ops = dynasmrt::x64::Assembler::new().unwrap();
    let mut labels = HashMap::new();
    let start = ops.offset();
    /*
        Register allocations:
        rdi: memory
        rsi: input
        rax: output
        r8: scratch
        r9: scratch
        (using .alias didn't work for some reason?)
    */
    dynasm!(ops
        ; .arch x64
    );
    let mut offset: usize = 0;
    while offset < memory.len() {
        translate(&memory, &mut offset, &mut ops, &mut labels);
    }
    dynasm!(ops
        ; ret
    );
    let code = ops.finalize().unwrap();
    let result: extern "sysv64" fn(*mut i64, i64) -> i64 = unsafe {
        mem::transmute(code.ptr(start))
    };
    mem::forget(code);
    move |input: i64| -> i64 {
        result(memory.as_mut_ptr(), input)
    }
}


#[cfg(test)]
mod test {
    use crate::compile;
    use std::num::Wrapping;

    #[test]
    fn test_output() {
        assert_eq!(compile("4,2,99")(0), 99); // pos
        assert_eq!(compile("104,100,99")(0), 100); // imm
    }

    #[test]
    fn test_input() {
        assert_eq!(compile("3,0,4,0,99")(-1234), -1234);
        assert_eq!(compile("3,5,4,5,99,0")(-1234), -1234);
    }

    #[test]
    fn test_add() {
        // -4 + -4 = -8
        assert_eq!(compile("3,0,1,0,0,1,4,1,99")(-4), -8); // pos, pos;
        // 10 + -5 = 5
        assert_eq!(compile("3,0,1101,10,-5,1,4,1,99")(0), 5); // imm, imm
        // 10 + -5 = 5
        assert_eq!(compile("3,0,1001,0,-5,1,4,1,99")(10), 5); // pos, imm
        // -5 + 10 = 5
        assert_eq!(compile("3,0,101,-5,0,1,4,1,99")(10), 5); // imm, pos
    }

    #[test]
    fn test_mul() {
        // -4 * -4 = 16
        assert_eq!(compile("3,0,2,0,0,1,4,1,99")(-4), 16); // pos, pos;
        // 10 * -5 = -50
        assert_eq!(compile("3,0,1102,10,-5,1,4,1,99")(0), -50); // imm, imm
        // 10 * -5 = -50
        assert_eq!(compile("3,0,1002,0,-5,1,4,1,99")(10), -50); // pos, imm
        // -5 * 10 = -50
        assert_eq!(compile("3,0,102,-5,0,1,4,1,99")(10), -50); // imm, pos
    }

    #[test]
    fn test_jump() {
        // jnz
        assert_eq!(compile("104,1,1105,1,7,104,100,99")(0), 1); // imm, imm
        assert_eq!(compile("104,1,1105,0,7,104,100,99")(0), 100); // imm, imm
        assert_eq!(compile("104,1,1005,1,7,104,100,99")(0), 1); // pos, imm
        assert_eq!(compile("104,0,1005,1,7,104,100,99")(0), 100); // pos, imm

        // jz
        assert_eq!(compile("104,1,1106,1,7,104,100,99")(0), 100); // imm, imm
        assert_eq!(compile("104,1,1106,0,7,104,100,99")(0), 1); // imm, imm
        assert_eq!(compile("104,1,1006,1,7,104,100,99")(0), 100); // pos, imm
        assert_eq!(compile("104,0,1006,1,7,104,100,99")(0), 0); // pos, imm
    }

    #[test]
    fn test_comparisons() {
        // cmp_less_than
        assert_eq!(compile("1107,1,1,0,4,0,99")(0), 0); // imm, imm
        assert_eq!(compile("1107,0,-1,0,4,0,99")(0), 0); // imm, imm
        assert_eq!(compile("1107,1,2,0,4,0,99")(0), 1); // imm, imm
        assert_eq!(compile("1107,-1,0,0,4,0,99")(0), 1); // imm, imm
        assert_eq!(compile("7,7,8,0,4,0,99,98,99")(0), 1); // pos, pos
        assert_eq!(compile("7,7,8,0,4,0,99,99,99")(0), 0); // pos, pos
        assert_eq!(compile("1007,7,99,0,4,0,99,98")(0), 1); // pos, imm
        assert_eq!(compile("1007,7,99,0,4,0,99,99")(0), 0); // pos, imm
        assert_eq!(compile("107,98,7,0,4,0,99,99")(0), 1); // imm, pos
        assert_eq!(compile("107,99,7,0,4,0,99,99")(0), 0); // imm, pos

        // cmp_equals
        assert_eq!(compile("1108,1,1,0,4,0,99")(0), 1); // imm, imm
        assert_eq!(compile("1108,1,2,0,4,0,99")(0), 0); // imm, imm
        assert_eq!(compile("8,7,8,0,4,0,99,99,99")(0), 1); // pos, pos
        assert_eq!(compile("8,7,8,0,4,0,99,99,999")(0), 0); // pos, pos
        assert_eq!(compile("1008,7,99,0,4,0,99,99")(0), 1); // pos, imm
        assert_eq!(compile("1008,7,98,0,4,0,99,99")(0), 0); // pos, imm
        assert_eq!(compile("108,99,7,0,4,0,99,99")(0), 1); // imm, pos
        assert_eq!(compile("108,98,7,0,4,0,99,99")(0), 0); // imm, pos
    }

    #[test]
    fn fibonacci() {
        fn fib_rust(n: i64) -> i64 {
            if n <= 2 {
                return 1;
            }
            let mut prev2 = Wrapping(1i64);
            let mut prev1 = Wrapping(1i64);
            let mut result = Wrapping(2i64);
            for _ in 2..n {
                result = prev1 + prev2;
                prev2 = prev1;
                prev1 = result;
            }
            result.0
        }
        /*
             0: add 0, 0, [0]           ; scratch space: 0: n, 1: prev2, 2: prev1, 3: result
             4: add 0, 0, [0]           ; scratch space: 4: cnt, 5: condition_tmp
             8: input [0]
            10: less_than [0], 3, [5]   ; if n <= 2 {
            14: jump_zero [5], 20
            17: output 1                ; return 1;
            19: halt                    ; }
            20: add 0, 1, [1]           ; prev2 = 1
            24: add 0, 1, [2]           ; prev1 = 1
            28: add 0, 1, [3]           ; result = 1
            32: add 0, 2, [4]           ; cnt = 2
            36: equals [4], [0], [5]    ; if cnt == n {
            40: jump_zero [5], 46
            43: output [3]              ; return result
            45: halt                    ; }
            46: add [1], [2], [3]       ; result = prev2 + prev1
            50: add [2], 0, [1]         ; prev2 = prev1
            54: add [3], 0, [2]         ; prev1 = result
            58: add [4], 1, [4]         ; count++
            52: jump_zero 0, 36         ; goto 36
        */
        let fib_intcode = "
            1101,0,0,0,
            1101,0,0,0,
            3,0,
            1007,0,3,5,
            1006,5,20,
            104,1,99,
            1101,0,1,1,
            1101,0,1,2,
            1101,0,1,3,
            1101,0,2,4,
            8,4,0,5,
            1006,5,46,
            4,3,99,
            1,1,2,3,
            1001,2,0,1,
            1001,3,0,2,
            1001,4,1,4,
            1106,0,36
        ";
        for i in 0..100 {
            assert_eq!(compile(fib_intcode)(i), fib_rust(i))
        }
    }
}
