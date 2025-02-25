use std::io;
use std::io::prelude::*;

use super::*;
use crate::error::Error;
use crate::reader::Offset;
use crate::value;
use crate::value::{Dict, Value};

// --- Op ----------------------------------------------------------------------

/**
Atomic operations.

Specifies atomic level operations like running a parsable structure or running
VM code.
*/
#[derive(Debug, Clone)]
pub enum Op {
    Nop,
    Offset(Box<Offset>), // Source offset position for debugging
    Rust(Rust),          // Native rust callback

    // Capture frames
    Frame(usize),   // Start new frame with optional forward fuse
    Commit,         // Commit frame
    Reset,          // Reset frame
    Close,          // Close frame
    Collect(usize), // Collect stack values from current frame
    Fuse(usize),    // Set frame fuse to forward address

    // Loop frames
    Loop(usize), // Loop frame
    Break,       // Ok(Accept::Break)
    LoadBreak,   // Ok(Accept::Break) with value
    Continue,    // Ok(Accept::Continue)

    // Conditional jumps
    ForwardIfTrue(usize),      // Jump forward when TOS is true
    ForwardIfFalse(usize),     // Jump forward when TOS is false
    ForwardIfConsumed(usize),  // Jump forward when frame consumed input
    BackwardIfTrue(usize),     // Jump backward when TOS is true
    BackwardIfFalse(usize),    // Jump backward when TOS is false
    BackwardIfConsumed(usize), // Jump backward when frame consumed input

    // Direct jumps
    Forward(usize),  // Jump forward
    Backward(usize), // Jump backward

    // Interrupts
    Skip,                  // Err(Reject::Skip)
    Next,                  // Err(Reject::Next)
    Push,                  // Ok(Accept::Push)
    LoadPush,              // Ok(Accept::Push) with value
    Accept,                // Ok(Accept::Return)
    LoadAccept,            // Ok(Accept::Return) with value
    Repeat,                // Ok(Accept::Repeat)
    LoadRepeat,            // Ok(Accept::Repeat) with value
    Reject,                // Ok(Err::Reject)
    LoadExit,              // Exit with errorcode
    Exit,                  // Exit with 0
    Error(Option<String>), // Error with optional error message (otherwise its expected on stack)

    // Call
    CallOrCopy,          // Load and eventually call stack element without parameters
    Call,                // Call stack element without parameters
    CallArg(usize),      // Call stack element with sequential parameters
    CallArgNamed(usize), // Call stack element with sequential and named parameters
    CallStatic(usize),   // Call static element without parameters
    CallStaticArg(Box<(usize, usize)>), // Call static element with sequential parameters
    CallStaticArgNamed(Box<(usize, usize)>), // Call static element with sequential and named parameters

    // Constants
    LoadStatic(usize), // Push a constant from the statics
    Push0,             // Push Integer(0)
    Push1,             // Push Integer(1)
    PushVoid,          // Push Void
    PushNull,          // Push Null
    PushTrue,          // Push True
    PushFalse,         // Push False

    // Variables & Values
    LoadGlobal(usize),
    LoadFast(usize),
    LoadFastCapture(usize),
    LoadCapture,
    LoadAttr,
    LoadIndex,
    //LoadFastAttr(usize),
    StoreGlobal(usize),
    StoreGlobalHold(usize),
    StoreFast(usize),
    StoreFastHold(usize),
    StoreFastCapture(usize),
    StoreFastCaptureHold(usize),
    StoreCapture,
    StoreCaptureHold,
    StoreIndex,
    StoreIndexHold,

    MakeAlias,       // Make key-value-Capture from last two stack items
    MakeDict(usize), // Make a Dict from specified amount of key-value-pairs

    // Operations
    Drop,  // drop TOS
    Clone, // clone TOS
    Dup,   // duplicate TOS
    Rot2,  // rotate TOS by 2

    Add, // binary add
    Sub, // binary sub
    Mul, // binary mul
    Div, // binary div

    Not, // unary not (! operator)
    Neg, // unary negation (- operator)

    InlineAdd, // Inline add (+= operator)
    InlineSub, // Inline sub (-= operator)
    InlineMul, // Inline mul (*= operator)
    InlineDiv, // Inline div (/= operator)

    InlineInc, // Inline increment (++x and x++ operators)
    InlineDec, // Inline decrement (--x and x-- operators)

    Equal,        // Compare for equality (== operator)
    NotEqual,     // Compare for unequality (!= operator)
    LowerEqual,   // Compare for lower-equality (<= operator)
    GreaterEqual, // Compare for greater-equality (>= operator)
    Lower,        // Compare for lowerness (< operator)
    Greater,      // Compare for greaterness (> operator)
}

impl Op {
    pub fn execute(ops: &[Op], context: &mut Context, debug: u8) -> Result<Accept, Reject> {
        if ops.len() == 0 {
            return Ok(Accept::Next);
        }

        fn dump(ops: &[Op], context: &Context, ip: usize) {
            for (i, op) in ops.iter().enumerate() {
                context.debug(&format!(
                    "{}{:03} {:?}",
                    if i == ip { ">" } else { " " },
                    i,
                    op
                ));
            }
        }

        // Frame ---------------------------------------------------------------
        #[derive(Debug)]
        struct Frame {
            fuse: Option<usize>,  // fuse
            capture_start: usize, // capture start
            reader_start: Offset, // reader start
        }

        impl Frame {
            // Creates a new frame from context.
            fn new(context: &Context) -> Frame {
                Frame {
                    fuse: None,
                    capture_start: context.runtime.stack.len(),
                    reader_start: context.runtime.reader.tell(),
                }
            }
        }

        // ---------------------------------------------------------------------

        let mut ip = 0; // Instruction pointer
        let mut frames: Vec<Frame> = Vec::new(); // Frames
        let mut loops: Vec<(usize, usize, usize)> = Vec::new(); // Loops

        let mut frame = Frame::new(context); // Main capture
        let mut state = Ok(Accept::Next);

        while ip < ops.len() {
            let op = &ops[ip];

            // Debug
            if debug == 3 {
                context.debug(&format!("{:03}:{}", ip, op));
            } else if debug > 3 {
                // Dump entire code
                context.debug("--- Code ---");
                dump(ops, context, ip);

                // Dump stack and frames
                if debug > 4 {
                    context.debug("--- Stack ---");
                    for i in 0..context.runtime.stack.len() {
                        context.debug(&format!(" {:03} {:?}", i, context.runtime.stack[i]));
                    }

                    context.debug("--- Frames ---");
                    for i in 0..frames.len() {
                        context.debug(&format!(" {:03} {:?}", i, frames[i]));
                    }

                    context.debug(&format!(" {:03} {:?}", frames.len(), frame));
                }

                // Step-by-step
                if debug > 5 {
                    let _ = io::stdin().read(&mut [0u8]).unwrap();
                }
            }

            // Execute instruction
            state = match op {
                Op::Nop => Ok(Accept::Next),

                Op::Offset(offset) => {
                    context.source_offset = Some(**offset);
                    Ok(Accept::Next)
                }

                Op::Rust(f) => f.0(context),

                // Frames
                Op::Frame(fuse) => {
                    frames.push(frame);
                    frame = Frame::new(context);

                    if *fuse > 0 {
                        frame.fuse = Some(ip + *fuse);
                    }

                    Ok(Accept::Next)
                }

                Op::Commit => {
                    frame.capture_start = context.runtime.stack.len();
                    frame.reader_start = context.runtime.reader.tell();
                    Ok(Accept::Next)
                }

                Op::Reset => {
                    context.runtime.stack.truncate(frame.capture_start);
                    context.runtime.reader.reset(frame.reader_start);
                    Ok(Accept::Next)
                }

                Op::Close => {
                    frame = frames.pop().unwrap();
                    Ok(Accept::Next)
                }

                Op::Collect(severity) => {
                    match context.collect(frame.capture_start, false, true, true, *severity as u8) {
                        Err(capture) => Ok(Accept::Push(capture)),
                        Ok(Some(value)) => Ok(Accept::Push(Capture::Value(value, None, 5))),
                        Ok(None) => Ok(Accept::Next),
                    }
                }

                Op::Fuse(addr) => {
                    frame.fuse = Some(ip + *addr);
                    Ok(Accept::Next)
                }

                // Loops
                Op::Loop(size) => {
                    frames.push(frame);
                    frame = Frame::new(context);
                    loops.push((frames.len(), ip, ip + *size));
                    Ok(Accept::Next)
                }

                Op::Break | Op::LoadBreak => {
                    let current = loops.pop().unwrap();

                    // Save value?
                    let value = if matches!(op, Op::LoadBreak) {
                        Some(context.pop())
                    } else {
                        None
                    };

                    // Discard all open frames inside current loop.
                    while frames.len() >= current.0 {
                        frame = frames.pop().unwrap();
                        context.runtime.stack.truncate(frame.capture_start);
                    }

                    // Jump behind loop
                    ip = current.2;

                    Ok(if let Some(value) = value {
                        Accept::Push(Capture::Value(value, None, 10))
                    } else {
                        Accept::Next
                    })
                }

                Op::Continue => {
                    let current = loops.last().unwrap();

                    // Discard all open frames inside current loop.
                    while frames.len() > current.0 {
                        frame = frames.pop().unwrap();
                    }

                    context.runtime.stack.truncate(frame.capture_start);

                    // Jump to loop start.
                    ip = current.1;
                    Ok(Accept::Next)
                }

                // Conditional jumps
                Op::ForwardIfTrue(goto) => {
                    if context.pop().is_true() {
                        ip += goto;
                    } else {
                        ip += 1;
                    }

                    Ok(Accept::Hold)
                }

                Op::ForwardIfFalse(goto) => {
                    if !context.pop().is_true() {
                        ip += goto;
                    } else {
                        ip += 1;
                    }

                    Ok(Accept::Hold)
                }

                Op::ForwardIfConsumed(goto) => {
                    if frame.reader_start != context.runtime.reader.tell() {
                        ip += goto;
                        Ok(Accept::Hold)
                    } else {
                        Ok(Accept::Next)
                    }
                }

                Op::BackwardIfTrue(goto) => {
                    if context.pop().is_true() {
                        ip -= goto;
                    } else {
                        ip += 1;
                    }

                    Ok(Accept::Hold)
                }

                Op::BackwardIfFalse(goto) => {
                    if !context.pop().is_true() {
                        ip -= goto;
                    } else {
                        ip += 1;
                    }

                    Ok(Accept::Hold)
                }

                Op::BackwardIfConsumed(goto) => {
                    if frame.reader_start != context.runtime.reader.tell() {
                        ip -= goto;
                        Ok(Accept::Hold)
                    } else {
                        Ok(Accept::Next)
                    }
                }

                Op::Forward(goto) => {
                    ip += goto;
                    Ok(Accept::Hold)
                }

                Op::Backward(goto) => {
                    ip -= goto;
                    Ok(Accept::Hold)
                }

                // Interrupts
                Op::Skip => Err(Reject::Skip),
                Op::Next => Err(Reject::Next),

                Op::Push => Ok(Accept::Push(Capture::Empty)),
                Op::LoadPush => {
                    let value = context.pop();
                    Ok(Accept::Push(Capture::Value(value, None, 15))) // high severity for override required here
                }

                Op::Accept => Ok(Accept::Return(None)),
                Op::LoadAccept => {
                    let value = context.pop();
                    Ok(Accept::Return(Some(value)))
                }
                Op::Repeat => Ok(Accept::Repeat(None)),
                Op::LoadRepeat => {
                    let value = context.pop();
                    Ok(Accept::Repeat(Some(value)))
                }
                Op::Reject => Err(Reject::Return),
                Op::LoadExit => {
                    std::process::exit(context.pop().to_i64() as i32);
                }
                Op::Exit => std::process::exit(0),

                Op::Error(msg) => {
                    if let Some(msg) = msg {
                        Error::new(Some(frame.reader_start), msg.clone()).into()
                    } else {
                        Error::new(Some(frame.reader_start), context.pop().to_string()).into()
                    }
                }

                // Calls
                Op::CallOrCopy => {
                    let value = context.pop();

                    if value.is_callable(false) {
                        // Call the value without parameters
                        value.call(context, 0, None)
                    } else {
                        // Push a copy of the value
                        context.push(value.borrow().clone().into())
                    }
                }

                Op::Call => {
                    let target = context.pop();
                    target.call(context, 0, None)
                }

                Op::CallArg(args) => {
                    let target = context.pop();
                    target.call(context, *args, None)
                }

                Op::CallArgNamed(args) => {
                    let target = context.pop();

                    if let Value::Dict(nargs) = context.pop().into() {
                        target.call(context, *args, Some(*nargs))
                    } else {
                        panic!("nargs operand required to be dict")
                    }
                }

                Op::CallStatic(addr) => {
                    context.runtime.program.statics[*addr].call(context, 0, None)
                }

                Op::CallStaticArg(addr_args) => {
                    context.runtime.program.statics[addr_args.0].call(context, addr_args.1, None)
                    //println!("CallStaticArg returns {:?}", ret);
                }

                Op::CallStaticArgNamed(addr_args) => {
                    if let Value::Dict(nargs) = context.pop().into() {
                        context.runtime.program.statics[addr_args.0].call(
                            context,
                            addr_args.1,
                            Some(*nargs),
                        )
                    } else {
                        panic!("nargs operand required to be dict")
                    }
                }

                // Variables and values
                Op::LoadStatic(addr) => {
                    let value = &context.runtime.program.statics[*addr];
                    context.push(value.borrow().clone().into())
                }
                Op::Push0 => context.push(value!(0 as i64)),
                Op::Push1 => context.push(value!(1 as i64)),
                Op::PushVoid => context.push(value!(void)),
                Op::PushNull => context.push(value!(null)),
                Op::PushTrue => context.push(value!(true)),
                Op::PushFalse => context.push(value!(false)),

                Op::LoadGlobal(addr) => context.load(*addr),
                Op::LoadFast(addr) => context.load(context.stack_start + *addr),

                Op::LoadFastCapture(index) => {
                    let value = context.get_capture(*index).unwrap_or(value!(void));
                    context.push(value)
                }

                Op::LoadCapture => {
                    let index = context.pop();
                    let index = index.borrow();

                    let value = if let Some(alias) = index.str() {
                        context.get_capture_by_name(alias).unwrap_or(value!(void))
                    } else {
                        context
                            .get_capture(index.to_usize())
                            .unwrap_or(value!(void))
                    };

                    context.push(value)
                }

                Op::LoadAttr => {
                    let attr = context.pop();
                    let attr = attr.borrow();
                    let value = context.pop();

                    match value.create_method(attr.str().unwrap()) {
                        Ok(value) => context.push(value),
                        Err(msg) => Error::new(None, msg).into(),
                    }
                }

                Op::LoadIndex => {
                    //fixme
                    /*
                    let index = context.pop();
                    let index = index.borrow();
                    let value = context.pop();
                    let value = value.borrow();

                    match value.get_index(&index) {
                        Ok(value) => context.push(value),
                        Err(msg) => Error::new(None, msg).into(),
                    }
                    */
                    todo!();
                }

                Op::StoreGlobal(addr) => {
                    // todo: bounds checking?
                    let value = context.pop();
                    context.runtime.stack[*addr] = Capture::Value(value, None, 0);
                    Ok(Accept::Next)
                }

                Op::StoreGlobalHold(addr) => {
                    // todo: bounds checking?
                    let value = context.peek();
                    context.runtime.stack[*addr] =
                        Capture::Value(value.borrow().clone().into(), None, 0);
                    Ok(Accept::Next)
                }

                Op::StoreFast(addr) => {
                    // todo: bounds checking?
                    let value = context.pop();
                    context.runtime.stack[context.stack_start + *addr] =
                        Capture::Value(value, None, 0);
                    Ok(Accept::Push(Capture::Empty))
                }

                Op::StoreFastHold(addr) => {
                    // todo: bounds checking?
                    let value = context.peek();
                    context.runtime.stack[context.stack_start + *addr] =
                        Capture::Value(value.borrow().clone().into(), None, 0);
                    Ok(Accept::Next)
                }

                Op::StoreFastCapture(index) => {
                    let value = context.pop();

                    context.set_capture(*index, value);
                    Ok(Accept::Push(Capture::Empty))
                }

                Op::StoreFastCaptureHold(index) => {
                    let value = context.peek();

                    context.set_capture(*index, value.borrow().clone().into());
                    Ok(Accept::Next)
                }

                Op::StoreCapture | Op::StoreCaptureHold => {
                    let index = context.pop();
                    let index = index.borrow();

                    if let Some(alias) = index.str() {
                        if matches!(op, Op::StoreCapture) {
                            let value = context.pop();
                            context.set_capture_by_name(alias, value);
                            Ok(Accept::Push(Capture::Empty))
                        } else {
                            let value = context.peek();
                            context.set_capture_by_name(alias, value.borrow().clone().into());
                            Ok(Accept::Next)
                        }
                    } else {
                        if matches!(op, Op::StoreCapture) {
                            let value = context.pop();
                            context.set_capture(index.to_usize(), value);
                            Ok(Accept::Push(Capture::Empty))
                        } else {
                            let value = context.peek();
                            context.set_capture(index.to_usize(), value.borrow().clone().into());
                            Ok(Accept::Next)
                        }
                    }
                }

                Op::StoreIndex | Op::StoreIndexHold => {
                    //fixme
                    /*
                    let index = context.pop();
                    let index = index.borrow();
                    let target = context.pop();
                    let value = context.pop();

                    let mut obj = target.borrow_mut();

                    if let Err(msg) = obj.set_index(&index, value) {
                        Error::new(None, msg).args[0].as_ref().unwrap().()
                    } else {
                        if matches!(op, Op::StoreIndexHold) {
                            context.push(target.clone())
                        } else {
                            Ok(Accept::Next)
                        }
                    }
                    */
                    todo!();
                }

                Op::MakeAlias => {
                    let name = context.pop();

                    match context.runtime.stack.last_mut().unwrap() {
                        Capture::Range(_, alias, ..) | Capture::Value(_, alias, ..) => {
                            *alias = Some(name.to_string());
                        }

                        empty => {
                            *empty = Capture::Value(value!(void), Some(name.to_string()), 0);
                        }
                    }

                    Ok(Accept::Next)
                }

                Op::MakeDict(count) => {
                    let mut dict = Dict::new();

                    for _ in 0..*count {
                        let key = context.pop();

                        let value = context.pop();
                        dict.insert(key.to_string(), value);
                    }

                    context.push(RefValue::from(dict))
                }

                // Operations
                Op::Drop => {
                    context.pop();
                    Ok(Accept::Next)
                }

                Op::Clone => {
                    let value = context.peek();
                    context.push(value.clone())
                }

                Op::Dup => {
                    let value = context.peek();
                    let value = value.borrow();
                    context.push(value.clone().into())
                }

                Op::Rot2 => {
                    let a = context.runtime.stack.pop().unwrap();
                    let b = context.runtime.stack.pop().unwrap();

                    context.runtime.stack.push(a);
                    context.runtime.stack.push(b);

                    Ok(Accept::Next)
                }

                // Operations
                Op::Add | Op::Sub | Op::Mul | Op::Div => {
                    let b = context.pop();
                    let a = context.pop();

                    /*
                    println!("{:?}", op);
                    println!("a = {:?}", a);
                    println!("b = {:?}", b);
                    */

                    let c = match op {
                        Op::Add => a.add(b)?.into(),
                        Op::Sub => a.sub(b)?.into(),
                        Op::Mul => a.mul(b)?.into(),
                        Op::Div => a.div(b)?.into(),
                        _ => unimplemented!("Unimplemented operator"),
                    };

                    context.push(c)
                }

                Op::Equal
                | Op::NotEqual
                | Op::LowerEqual
                | Op::GreaterEqual
                | Op::Lower
                | Op::Greater => {
                    let b = context.pop();
                    let a = context.pop();

                    //println!("{:?}", op);
                    //println!("a = {:?}", a);
                    //println!("b = {:?}", b);

                    let c = match op {
                        Op::Equal => a == b,
                        Op::NotEqual => a != b,
                        Op::LowerEqual => a <= b,
                        Op::GreaterEqual => a >= b,
                        Op::Lower => a < b,
                        Op::Greater => a > b,

                        _ => unimplemented!("Unimplemented operator"),
                    };

                    //println!("c = {:?}", c);

                    context.push(RefValue::from(c))
                }

                Op::Not => {
                    let value = context.pop().not()?.into();
                    context.push(value)
                }
                Op::Neg => {
                    let value = context.pop().neg()?.into();
                    context.push(value)
                }
                Op::InlineAdd | Op::InlineSub | Op::InlineMul | Op::InlineDiv => {
                    let b = context.pop();
                    let value = context.pop();

                    /*
                    println!("{:?}", op);
                    println!("a = {:?}", a);
                    println!("b = {:?}", b);
                    */

                    let res = match op {
                        Op::InlineAdd => value.add(b)?,
                        Op::InlineSub => value.sub(b)?,
                        Op::InlineMul => value.mul(b)?,
                        Op::InlineDiv => value.div(b)?,
                        _ => unimplemented!("Unimplemented operator"),
                    };

                    *value.borrow_mut() = res.into();

                    context.push(value.clone().into())
                }

                Op::InlineInc => {
                    let value = context.pop();

                    let res = value.add(value!(1 as i64))?; // todo: perform inc by bit-shift
                    *value.borrow_mut() = res.into();

                    context.push(value.clone().into())
                }

                Op::InlineDec => {
                    let value = context.pop();

                    let res = value.sub(value!(1 as i64))?; // todo: perform dec by bit-shift
                    *value.borrow_mut() = res.into();

                    context.push(value.clone().into())
                }
            };

            // Debug
            if context.runtime.debug > 3 {
                context.debug(&format!("ip = {} state = {:?}", ip, state));
            }

            match state {
                Ok(Accept::Hold) => {}
                Ok(Accept::Next) => ip += 1,
                Ok(Accept::Push(capture)) => {
                    context.runtime.stack.push(capture);
                    state = Ok(Accept::Next);
                    ip += 1;
                }
                Err(Reject::Next) if frames.len() > 0 => loop {
                    context.runtime.stack.truncate(frame.capture_start);
                    context.runtime.reader.reset(frame.reader_start);

                    if let Some(fuse) = frame.fuse {
                        if fuse > ip {
                            ip = fuse;
                            break;
                        }
                    }

                    if frames.len() == 0 {
                        return Err(Reject::Next);
                    }

                    frame = frames.pop().unwrap();
                },
                _ => {
                    return state;
                }
            }
        }

        state
    }
}

impl std::fmt::Display for Op {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Op::Rust(_) => write!(f, "{{rust-function}}"),
            op => write!(f, "{:?}", op),
        }
    }
}

#[derive(Clone)]
pub struct Rust(pub fn(&mut Context) -> Result<Accept, Reject>);

impl std::fmt::Debug for Rust {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{{rust-function}}")
    }
}
