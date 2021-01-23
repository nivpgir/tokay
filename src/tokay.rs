use std::collections::HashMap;
use std::cell::RefCell;
use std::rc::Rc;
use std::iter::FromIterator;

use crate::ccl::Ccl;
use crate::value::{Dict, List, Value, RefValue};
use crate::reader::{Reader, Range};
use crate::compiler::Compiler;
use crate::ccl;


#[derive(Debug, Clone)]
pub enum Accept {
    Next,
    Skip,
    Push(Capture),
    Repeat(Option<RefValue>),
    Return(Option<RefValue>)
}


#[derive(Debug, Clone)]
pub enum Reject {
    Next,
    Return,
    Main,
    Error(String)
}


/** Parser trait */
pub trait Parser: std::fmt::Debug + std::fmt::Display {
    /** Perform a parse on a given context.

    A parse may either Accept or Reject, with a given severity.
    */
    fn run(&self, context: &mut Context) -> Result<Accept, Reject>;

    /** Finalize according grammar view;

    This function is called from top of each parselet to detect
    both left-recursive and nullable (=no input consuming) structures. */
    fn finalize(
        &mut self,
        _statics: &Vec<RefValue>,
        _leftrec: &mut bool,
        _nullable: &mut bool)
    {
        // default is: just do nothing ;)
    }

    /** Resolve is called by the compiler to resolve unresolved symbols
    inside or below a program structure */
    fn resolve(
        &mut self,
        _compiler: &Compiler,
        _locals: bool,
        _strict: bool)
    {
        // default is: just do nothing ;)
    }

    /** Convert parser object into boxed dyn Parser Op */
    fn into_op(self) -> Op
        where Self: std::marker::Sized + 'static
    {
        Op::Parser(Box::new(self))
    }
}


// --- Op ----------------------------------------------------------------------

/**
Atomic operations.

Specifies atomic level operations like running a parser or running VM code.
*/
#[derive(Debug)]
pub enum Op {
    Nop,

    // Parsing
    Parser(Box<dyn Parser>),

    Empty,
    Peek(Box<Op>),          // Peek-operation
    Not(Box<Op>),           // Not-predicate

    // Call
    Symbol(String),
    TryCall,
    Call,
    CallStatic(usize),

    // Debuging and error reporting
    Print,                  // todo: make this a builtin
    Debug(&'static str),    // todo: make this a builtin
    Error(&'static str),    // todo: make this a builtin
    Expect(Box<Op>),        // todo: make this a builtin

    // AST construction
    Create(&'static str),   // todo: make this a builtin
    Lexeme(&'static str),   // todo: make this a builtin

    // Interrupts
    Skip,
    LoadAccept,
    Reject,

    // Constants
    LoadStatic(usize),
    PushTrue,
    PushFalse,
    PushVoid,

    // Variables & Values
    LoadGlobal(usize),
    LoadFast(usize),
    StoreGlobal(usize),
    StoreFast(usize),
    LoadFastCapture(usize),
    LoadCapture,
    StoreFastCapture(usize),
    StoreCapture,

    // Operations
    Add,
    Sub,
    Div,
    Mul
}

impl Op {
    pub fn into_box(self) -> Box<Self> {
        Box::new(self)
    }

    pub fn into_kleene(self) -> Self {
        Repeat::kleene(self)
    }

    pub fn into_positive(self) -> Self {
        Repeat::positive(self)
    }

    pub fn into_optional(self) -> Self {
        Repeat::optional(self)
    }
}

impl Parser for Op {
    fn run(&self, context: &mut Context) -> Result<Accept, Reject> {
        match self {
            Op::Nop => Ok(Accept::Next),

            Op::Parser(p) => p.run(context),

            Op::Symbol(_) => panic!("{:?} cannot be called", self),

            Op::TryCall => {
                let value = context.pop();
                if value.borrow().is_callable() {
                    value.borrow().call(context)
                }
                else {
                    Ok(Accept::Push(Capture::Value(value.clone(), 1)))
                }
            }

            Op::Call => {
                let value = context.pop();
                let value = value.borrow();
                value.call(context)
            }

            Op::CallStatic(addr) => {
                context.runtime.program.statics[*addr].borrow().call(context)
            }

            Op::Empty => {
                Ok(Accept::Push(Capture::Empty))
            }

            Op::Peek(p) => {
                let reader_start = context.runtime.reader.tell();
                let ret = p.run(context);
                context.runtime.reader.reset(reader_start);
                ret
            }

            Op::Not(p) => {
                if p.run(context).is_ok() {
                    Err(Reject::Next)
                }
                else {
                    Ok(Accept::Next)
                }
            }

            Op::Print => {
                let value = context.collect(
                    context.capture_start, true, false
                );

                if value.is_some() {
                    println!("{:?}", value.unwrap());
                }

                Ok(Accept::Next)
            },

            Op::Debug(s) => {
                println!("{}", s);
                Ok(Accept::Next)
            },

            Op::Error(s) => {
                Err(Reject::Error(s.to_string()))
            },

            Op::Expect(op) => {
                op.run(context).or_else(|_| {
                    Err(
                        Reject::Error(
                            format!("Expecting {}", op)
                        )
                    )
                })
            },

            Op::Create(emit) => {
                /*
                println!("Create {} from {:?}",
                    emit, &context.runtime.stack[context.capture_start..]
                );
                */

                let value = match context.collect(
                    context.capture_start, false, false)
                {
                    Some(capture) => {
                        let value = capture.as_value(context.runtime);
                        let mut ret = Dict::new();

                        ret.insert(
                            "emit".to_string(),
                            Value::String(emit.to_string()).into_ref()
                        );

                        // List or Dict values are classified as child nodes
                        if value.borrow().get_list().is_some()
                            || value.borrow().get_dict().is_some()
                        {
                            ret.insert(
                                "children".to_string(),
                                value
                            );
                        }
                        else {
                            ret.insert(
                                "value".to_string(),
                                value
                            );
                        }

                        Value::Dict(Box::new(ret)).into_ref()
                    }
                    None => {
                        Value::String(emit.to_string()).into_ref()
                    }
                };

                //println!("Create {} value = {:?}", emit, value);

                Ok(Accept::Return(Some(value)))
            },

            Op::Lexeme(emit) => {
                let value = Value::String(
                    context.runtime.reader.extract(
                        &context.runtime.reader.capture_from(
                            context.reader_start
                        )
                    )
                );

                let mut ret = Dict::new();

                ret.insert(
                    "emit".to_string(),
                    Value::String(emit.to_string()).into_ref()
                );

                ret.insert(
                    "value".to_string(),
                    value.into_ref()
                );

                Ok(
                    Accept::Return(
                        Some(Value::Dict(Box::new(ret)).into_ref())
                    )
                )
            },

            Op::Skip => {
                Ok(Accept::Skip)
            },

            Op::LoadAccept => {
                let value = context.pop();
                Ok(Accept::Return(Some(value.clone())))
            }

            /*
            Op::Accept(value) => {
                Ok(Accept::Return(value.clone()))
            },

            Op::Repeat(value) => {
                Ok(Accept::Repeat(value.clone()))
            },
            */

            Op::Reject => {
                Err(Reject::Return)
            },

            Op::LoadStatic(addr) => {
                Ok(Accept::Push(Capture::Value(
                    context.runtime.program.statics[*addr].clone(), 5
                )))
            }

            Op::PushTrue => {
                Ok(Accept::Push(
                    Capture::Value(Value::True.into_ref(), 5)
                ))
            },
            Op::PushFalse => {
                Ok(Accept::Push(
                    Capture::Value(Value::False.into_ref(), 5)
                ))
            },
            Op::PushVoid => {
                Ok(Accept::Push(
                    Capture::Value(Value::Void.into_ref(), 5)
                ))
            },

            Op::LoadGlobal(addr) => {
                Ok(Accept::Push(
                    Capture::Value(
                        context.runtime.stack[*addr]
                            .as_value(&context.runtime), 5
                    )
                ))
            },

            Op::LoadFast(addr) => {
                Ok(Accept::Push(
                    Capture::Value(
                        context.runtime.stack[
                            context.stack_start + addr
                        ].as_value(&context.runtime), 5
                    )
                ))
            },

            Op::StoreGlobal(addr) => {
                // todo
                Ok(Accept::Next)
            },

            Op::StoreFast(addr) => {
                context.runtime.stack[context.stack_start + addr] =
                    Capture::Value(context.pop(), 10);

                Ok(Accept::Next)
            },

            Op::LoadFastCapture(index) => {
                let value = context.get_capture(*index).unwrap_or(
                    Value::Void.into_ref()
                );

                Ok(Accept::Push(Capture::Value(value, 10)))
            },

            Op::LoadCapture => {
                let index = context.pop();
                let index = index.borrow();

                match *index {
                    Value::Addr(_)
                    | Value::Integer(_)
                    | Value::Float(_) => {
                        Op::LoadFastCapture(index.to_addr()).run(context)
                    }

                    _ => {
                        unimplemented!("//todo")
                    }
                }
            },

            Op::StoreFastCapture(index) => {
                let value = context.pop();

                context.set_capture(*index, value);
                Ok(Accept::Next)
            },

            Op::StoreCapture => {
                let index = context.pop();
                let index = index.borrow();

                match *index {
                    Value::Addr(_)
                    | Value::Integer(_)
                    | Value::Float(_) => {
                        Op::StoreFastCapture(index.to_addr()).run(context)
                    }

                    _ => {
                        unimplemented!("//todo")
                    }
                }
            },

            Op::Add | Op::Sub | Op::Div | Op::Mul => {
                let b = context.pop();
                let a = context.pop();

                /*
                println!("{:?}", self);
                println!("a = {:?}", a);
                println!("b = {:?}", b);
                */

                let c = match self {
                    Op::Add => (&*a.borrow() + &*b.borrow()).into_ref(),
                    Op::Sub => (&*a.borrow() - &*b.borrow()).into_ref(),
                    Op::Div => (&*a.borrow() / &*b.borrow()).into_ref(),
                    Op::Mul => (&*a.borrow() * &*b.borrow()).into_ref(),
                    _ => unimplemented!("Unimplemented operator")
                };

                Ok(Accept::Push(Capture::Value(c, 10)))
            }
        }
    }

    fn finalize(
        &mut self,
        statics: &Vec<RefValue>,
        leftrec: &mut bool,
        nullable: &mut bool)
    {
        match self {
            Op::Parser(parser) => parser.finalize(statics, leftrec, nullable),

            Op::Peek(op) | Op::Not(op) => op.finalize(statics, leftrec, nullable),

            Op::Symbol(_) => panic!("{:?} cannot be finalized", self),

            Op::CallStatic(addr) => {
                if let Value::Parselet(parselet) = &*statics[*addr].borrow()
                {
                    if let Ok(mut parselet)= parselet.try_borrow_mut()
                    {
                        let mut my_leftrec = parselet.leftrec;
                        let mut my_nullable = parselet.nullable;

                        parselet.body.finalize(
                            statics,
                            &mut my_leftrec,
                            &mut my_nullable,
                        );

                        parselet.leftrec = my_leftrec;
                        parselet.nullable = my_nullable;

                        *nullable = parselet.nullable;
                    }
                    else {
                        *leftrec = true;
                    }
                }
            },

            _ => {}
        }
    }

    fn resolve(
        &mut self,
        compiler: &Compiler,
        locals: bool,
        strict: bool)
    {
        match self {
            Op::Parser(parser) => parser.resolve(compiler, locals, strict),

            Op::Peek(op) | Op::Not(op) => op.resolve(compiler, locals, strict),

            Op::Symbol(name) => {
                // Resolve constants
                if let Some(addr) = compiler.get_constant(name) {
                    *self = Op::CallStatic(addr);
                    return;
                }

                if locals {
                    if let Some(addr) = compiler.get_local(name) {
                        *self = Sequence::new(vec![
                            (Op::LoadFast(addr), None),
                            (Op::TryCall, None)
                        ]);
                        return;
                    }
                }

                if let Some(addr) = compiler.get_global(name) {
                    *self = Sequence::new(vec![
                        (Op::LoadGlobal(addr), None),
                        (Op::TryCall, None)
                    ]);
                    return;
                }

                if !strict {
                    return;
                }

                panic!("Cannot resolve {:?}", name);
            }
            _ => {}
        }
    }
}

impl std::fmt::Display for Op {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Op::Parser(p) => write!(f, "{}", p),
            _ => write!(f, "Op #todo")
        }
    }
}

// --- Call --------------------------------------------------------------------
/*
#[derive(Debug)]
pub enum Unresolved {
    Symbol(String),
    Call{
        name: String,
        params: Vec<(Option<String>, Vec<Op>)>
    }
}

impl Parser for Unresolved {

    fn run(&self, context: &mut Context) -> Result<Accept, Reject> {
        panic!("Cannot run {:?}", self)
    }

    fn finalize(
        &mut self,
        _statics: &Vec<RefValue>,
        _leftrec: &mut bool,
        _nullable: &mut bool)
    {
        panic!("Cannot finalize {:?}", self)
    }

    fn resolve(
        &mut self,
        compiler: &Compiler,
        locals: bool,
        strict: bool)
    {
        if let Unresolved::Symbol(name) = self {
            // Resolve constants
            if let Some(value) = compiler.get_constant(name) {
                match &*value.borrow() {
                    Value::Parselet(p) => {
                        //println!("resolved {:?} as {:?}", name, *p);
                        *self = Call::Parselet(*p, params.drain(..).collect());
                        return;
                    },
                    Value::Builtin(b) => {
                        *self = Call::Builtin(*b, params.drain(..).collect());
                        return;
                    },

                    /* TODO!!!
                    Value::String(s) => {
                        resolved = Some(
                            Match::new(&s.clone()).into_op()
                        );
                    },

                    _ => {
                        resolved = Some(
                            Op::LoadStatic(
                                compiler.define_static(value.clone())
                            )
                        );
                    }
                    */
                    _ => panic!("#todo!")
                }
            }

            if locals {
                if let Some(addr) = compiler.get_local(name) {
                    params.push(Op::LoadFast(addr));
                    *self = Call::Dynamic(params.drain(..).collect());
                    return;
                }
            }

            if let Some(addr) = compiler.get_global(name) {
                params.push(Op::LoadFast(addr));
                *self = Call::Dynamic(params.drain(..).collect());
                return;
            }

            if !strict {
                return;
            }

            panic!("Cannot resolve {:?}", name);
        }
        else {
            unimplemented!("Unresolved::Call is MISSING!")
        }
    }
}

impl std::fmt::Display for Unresolved {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            _ => write!(f, "Unresolved #todo")
        }
    }
}
*/

// --- Rust --------------------------------------------------------------------

/** This is not really a parser, but it allows to run any Rust code in position
of a parser. */

pub struct Rust(pub fn(&mut Context) -> Result<Accept, Reject>);

impl Parser for Rust {
    fn run(&self, context: &mut Context) -> Result<Accept, Reject> {
        self.0(context)
    }
}

impl std::fmt::Debug for Rust {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{{rust-function}}")
    }
}

impl std::fmt::Display for Rust {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{{rust-function}}")
    }
}


// --- Char --------------------------------------------------------------------

/** Char parser.

This parser either matches simple characters or matches ranges until a specific
character is found.
*/

#[derive(Debug)]
pub struct Char {
    accept: Ccl,
    repeats: bool,
    silent: bool
}

impl Char {
    fn _new(accept: Ccl, repeats: bool, silent: bool) -> Op {
        Self{
            accept,
            repeats,
            silent
        }.into_op()
    }

    pub fn new_silent(accept: Ccl) -> Op {
        Self::_new(accept, false, true)
    }

    pub fn new(accept: Ccl) -> Op {
        Self::_new(accept, false, false)
    }

    pub fn any() -> Op {
        let mut any = Ccl::new();
        any.negate();

        Self::new_silent(any)
    }

    pub fn char(ch: char) -> Op {
        Self::new_silent(ccl![ch..=ch])
    }

    pub fn span(ccl: Ccl) -> Op {
        Self::_new(ccl, true, false)
    }

    pub fn until(ch: char) -> Op {
        let mut other = ccl![ch..=ch];
        other.negate();

        Self::span(other)
    }
}

impl Parser for Char {
    fn run(&self, context: &mut Context) -> Result<Accept, Reject> {
        let start = context.runtime.reader.tell();

        while let Some(ch) = context.runtime.reader.peek() {
            if !self.accept.test(&(ch..=ch)) {
                break;
            }

            context.runtime.reader.next();

            if !self.repeats {
                break;
            }
        }

        if start < context.runtime.reader.tell() {
            Ok(
                Accept::Push(
                    Capture::Range(
                        context.runtime.reader.capture_from(start), 5
                    )
                )
            )
        }
        else {
            context.runtime.reader.reset(start);
            Err(Reject::Next)
        }
    }

    fn finalize(
        &mut self,
        _statics: &Vec<RefValue>,
        _leftrec: &mut bool,
        nullable: &mut bool)
    {
        *nullable = false;
    }
}

impl std::fmt::Display for Char {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Char #todo")
    }
}

// --- Match -------------------------------------------------------------------

/** Match parser.

This parser implements the recognition of an exact character sequence within
the input stream.
*/

#[derive(Debug)]
pub struct Match{
    string: String,
    silent: bool
}

impl Match {
    pub fn new(string: &str) -> Op {
        Self{
            string: string.to_string(),
            silent: false
        }.into_op()
    }

    pub fn new_silent(string: &str) -> Op {
        Self{
            string: string.to_string(),
            silent: true
        }.into_op()
    }
}

impl Parser for Match {

    fn run(&self, context: &mut Context) -> Result<Accept, Reject> {
        let start = context.runtime.reader.tell();

        for ch in self.string.chars() {
            if let Some(c) = context.runtime.reader.next() {
                if c != ch {
                    // fixme: Optimize me!
                    context.runtime.reader.reset(start);
                    return Err(Reject::Next);
                }
            }
            else {
                // fixme: Optimize me!
                context.runtime.reader.reset(start);
                return Err(Reject::Next);
            }
        }

        let range = context.runtime.reader.capture_last(self.string.len());

        Ok(
            Accept::Push(
                if self.silent {
                    Capture::Range(
                        range, 0
                    )
                }
                else {
                    Capture::Range(
                        range, 5
                    )
                }
            )
        )
    }

    fn finalize(
        &mut self,
        _statics: &Vec<RefValue>,
        _leftrec: &mut bool,
        nullable: &mut bool)
    {
        *nullable = false;
    }
}

impl std::fmt::Display for Match {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.silent {
            write!(f, "'{}'", self.string)
        }
        else {
            write!(f, "\"{}\"", self.string)
        }
    }
}

// --- Repeat ------------------------------------------------------------------

/** Repeating parser.

This is a simple programmatic sequential repetition. For several reasons,
repetitions can also be expressed on a specialized token-level or by the grammar
itself using left- and right-recursive structures, resulting in left- or right-
leaning parse trees.
*/

#[derive(Debug)]
pub struct Repeat {
    parser: Op,
    min: usize,
    max: usize,
    silent: bool
}

impl Repeat {
    pub fn new(parser: Op, min: usize, max: usize, silent: bool) -> Op
    {
        assert!(max == 0 || max >= min);

        Self{
            parser,
            min,
            max,
            silent
        }.into_op()
    }

    pub fn kleene(parser: Op) -> Op {
        Self::new(parser, 0, 0, false)
    }

    pub fn positive(parser: Op) -> Op {
        Self::new(parser, 1, 0, false)
    }

    pub fn optional(parser: Op) -> Op {
        Self::new(parser, 0, 1, false)
    }

    pub fn kleene_silent(parser: Op) -> Op {
        Self::new(parser, 0, 0, true)
    }

    pub fn positive_silent(parser: Op) -> Op {
        Self::new(parser, 1, 0, true)
    }

    pub fn optional_silent(parser: Op) -> Op {
        Self::new(parser, 0, 1, true)
    }
}

impl Parser for Repeat {

    fn run(&self, context: &mut Context) -> Result<Accept, Reject> {
        // Remember capturing positions
        let capture_start = context.runtime.stack.len();
        let reader_start = context.runtime.reader.tell();

        let mut count: usize = 0;

        loop {
            match self.parser.run(context) {
                Err(Reject::Next) => break,

                Err(reject) => {
                    context.runtime.stack.truncate(capture_start);
                    context.runtime.reader.reset(reader_start);
                    return Err(reject)
                },

                Ok(Accept::Next) => {},

                Ok(Accept::Push(capture)) => {
                    if !self.silent {
                        context.runtime.stack.push(capture)
                    }
                },

                Ok(accept) => {
                    return Ok(accept)
                }
            }

            count += 1;

            if self.max > 0 && count == self.max {
                break
            }
        }

        if count < self.min {
            context.runtime.stack.truncate(capture_start);
            context.runtime.reader.reset(reader_start);
            Err(Reject::Next)
        }
        else {
            // Push collected captures, if any
            if let Some(capture) = context.collect(capture_start, false, false)
            {
                Ok(Accept::Push(capture))
            }
            // Otherwiese, push a capture of consumed range
            else if reader_start < context.runtime.reader.tell() {
                Ok(Accept::Push(
                    Capture::Range(
                        context.runtime.reader.capture_from(reader_start), 0
                    )
                ))
            }
            // Else, just accept next
            else {
                Ok(Accept::Next)
            }
        }
    }

    fn finalize(
        &mut self,
        statics: &Vec<RefValue>,
        leftrec: &mut bool,
        nullable: &mut bool)
    {
        self.parser.finalize(statics, leftrec, nullable);

        if self.min == 0 {
            *nullable = true;
        }
    }

    fn resolve(
        &mut self,
        compiler: &Compiler,
        locals: bool,
        strict: bool)
    {
        self.parser.resolve(compiler, locals, strict);
    }
}

impl std::fmt::Display for Repeat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Repeat #todo")
    }
}

// --- Sequence ----------------------------------------------------------------

/** Sequence parser.

This parser collects a sequence of sub-parsers. According to the sub-parsers
semantics, or when an entire sequence was completely recognized, the sequence
is getting accepted. Incomplete sequences are rejected.
*/

#[derive(Debug)]
pub struct Sequence {
    leftrec: bool,
    nullable: bool,
    items: Vec<(Op, Option<String>)>
}

impl Sequence {
    pub fn new(items: Vec<(Op, Option<String>)>) -> Op
    {
        Self{
            leftrec: false,
            nullable: true,
            items
        }.into_op()
    }
}

impl Parser for Sequence {

    fn run(&self, context: &mut Context) -> Result<Accept, Reject> {
        // Empty sequence?
        if self.items.len() == 0 {
            return Ok(Accept::Next);
        }

        // Remember capturing positions
        let capture_start = context.runtime.stack.len();
        let reader_start = context.runtime.reader.tell();

        // Iterate over sequence
        for (item, alias) in &self.items {
            match item.run(context) {
                Err(reject) => {
                    context.runtime.stack.truncate(capture_start);
                    context.runtime.reader.reset(reader_start);
                    return Err(reject);
                }

                Ok(Accept::Next) => {
                    if let Some(alias) = alias {
                        context.runtime.stack.push(
                            Capture::Named(
                                Box::new(Capture::Empty), alias.clone()
                            )
                        )
                    }
                    else {
                        context.runtime.stack.push(Capture::Empty)
                    }
                },

                Ok(Accept::Push(capture)) => {
                    if let Some(alias) = alias {
                        context.runtime.stack.push(
                            Capture::Named(Box::new(capture), alias.clone())
                        )
                    }
                    else {
                        context.runtime.stack.push(capture)
                    }
                },

                other => {
                    return other
                }
            }
        }

        /*
            When no explicit Return is performed, first try to collect any
            non-silent captures.
        */
        if let Some(capture) = context.collect(capture_start, false, true) {
            Ok(Accept::Push(capture))
        }
        /*
            When this fails, push a silent range of the current sequence
            when input was consumed.
        */
        else if reader_start < context.runtime.reader.tell() {
            Ok(
                Accept::Push(
                    Capture::Range(
                        context.runtime.reader.capture_from(reader_start), 0
                    )
                )
            )
        }
        /*
            Otherwise, just return Next.
        */
        else {
            Ok(Accept::Next)
        }
    }

    fn finalize(
        &mut self,
        statics: &Vec<RefValue>,
        leftrec: &mut bool,
        nullable: &mut bool)
    {
        for (item, _) in self.items.iter_mut() {
            item.finalize(
                statics,
                &mut self.leftrec,
                &mut self.nullable
            );

            if !self.nullable {
                break
            }
        }

        *leftrec = self.leftrec;
        *nullable = self.nullable;
    }

    fn resolve(
        &mut self,
        compiler: &Compiler,
        locals: bool,
        strict: bool)
    {
        for (item, _) in self.items.iter_mut() {
            item.resolve(compiler, locals, strict);
        }
    }

}

impl std::fmt::Display for Sequence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Sequence #todo")
    }
}

// --- Block -------------------------------------------------------------------

/** Block parser.

A block parser defines either an alternation of sequences or a grouped sequence
of VM instructions. The compiler has to guarantee for correct usage of the block
parser.

Block parsers support static program constructs being left-recursive, and extend
the generated parse tree automatically until no more input can be consumed.
*/

#[derive(Debug)]
pub struct Block {
    leftrec: bool,
    all_leftrec: bool,
    items: Vec<(Op, bool)>
}

impl Block {
    pub fn new(items: Vec<Op>) -> Op {
        Self{
            items: items.into_iter().map(|item| (item, false)).collect(),
            all_leftrec: false,
            leftrec: false
        }.into_op()
    }
}

impl Parser for Block {

    fn run(&self, context: &mut Context) -> Result<Accept, Reject>
    {
        // Internal Block run function
        fn run(block: &Block, context: &mut Context, leftrec: bool)
                -> Result<Accept, Reject>
        {
            let mut res = Ok(Accept::Next);
            let reader_start = context.runtime.reader.tell();

            for (item, item_leftrec) in &block.items {
                // Skip over parsers that don't match leftrec configuration
                if *item_leftrec != leftrec {
                    continue;
                }

                res = item.run(context);

                // Generally break on anything which is not Next.
                if !matches!(&res, Ok(Accept::Next) | Err(Reject::Next)) {
                    // Push only accepts when input was consumed, otherwise the
                    // push value is just discarded, except for the last item
                    // being executed.
                    if let Ok(Accept::Push(_)) = res {
                        // No consuming, no breaking!
                        if reader_start == context.runtime.reader.tell() {
                            continue
                        }
                    }

                    break
                }
            }

            res
        }

        // Create a unique block id from the Block's address
        let id = self as *const Block as usize;

        // Check for an existing memo-entry, and return it in case of a match
        if let Some((reader_end, result)) =
            context.runtime.memo.get(&(context.reader_start, id))
        {
            context.runtime.reader.reset(*reader_end);
            return result.clone();
        }

        if self.leftrec {
            //println!("Leftrec {:?}", self);

            // Left-recursive blocks are called in a loop until no more input
            // is consumed.

            let mut reader_end = context.reader_start;
            let mut result = if self.all_leftrec {
                Ok(Accept::Next)
            }
            else {
                Err(Reject::Next)
            };

            // Insert a fake memo entry to avoid endless recursion

            /* info: removing this fake entry does not affect program run!

            This is because of the leftrec parameter to internal run(),
            which only accepts non-left-recursive calls on the first run.
            As an additional fuse, this fake memo entry should anyway be kept.
            */
            context.runtime.memo.insert(
                (context.reader_start, id),
                (reader_end, result.clone())
            );

            let mut loops = 0;

            loop {
                let res = run(self, context, self.all_leftrec || loops > 0);

                match res {
                    // Hard reject
                    Err(Reject::Main) | Err(Reject::Error(_)) => {
                        return res
                    },

                    // Soft reject
                    Err(_) => {
                        if loops == 0 {
                            return res
                        }
                        else {
                            break
                        }
                    },

                    _ => {}
                }

                // Stop also when no more input was consumed
                if context.runtime.reader.tell() <= reader_end {
                    break
                }

                result = res;

                // Save intermediate result in memo table
                reader_end = context.runtime.reader.tell();
                context.runtime.memo.insert(
                    (context.reader_start, id),
                    (reader_end, result.clone())
                );

                // Reset reader & stack
                context.runtime.reader.reset(context.reader_start);
                context.runtime.stack.truncate(context.stack_start);
                context.runtime.stack.resize(
                    context.capture_start + 1,
                    Capture::Empty
                );

                loops += 1;
            }

            context.runtime.reader.reset(reader_end);
            result
        }
        else {
            // Non-left-recursive block can be called directly.
            run(self, context, false)
        }
    }

    fn finalize(
        &mut self,
        statics: &Vec<RefValue>,
        leftrec: &mut bool,
        nullable: &mut bool)
    {
        *nullable = false;
        self.all_leftrec = true;

        for (item, item_leftrec) in self.items.iter_mut() {
            *item_leftrec = false;
            let mut my_nullable = true;

            item.finalize(
                statics,
                item_leftrec,
                &mut my_nullable
            );

            if my_nullable {
                *nullable = true;
            }

            if *item_leftrec {
                self.leftrec = true;
            }
            else {
                self.all_leftrec = false;
            }
        }

        *leftrec = self.leftrec;
    }

    fn resolve(
        &mut self,
        compiler: &Compiler,
        locals: bool,
        strict: bool)
    {
        for (item, _) in self.items.iter_mut() {
            item.resolve(compiler, locals, strict);
        }
    }
}

impl std::fmt::Display for Block {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Block #todo")
    }
}

// --- Parselet ----------------------------------------------------------------

/** Parselet is the conceptual building block of Tokay.

A parselet is like a function in ordinary programming languages, with the
exception that it can either be a snippet of parsing instructions combined with
semantic code, or just an ordinary function consisting of code and returning
values. In general, the destinction if a parselet is a just a function or "more"
is defined by the parselets instruction set.
*/

#[derive(Debug)]
pub struct Parselet {
    leftrec: bool,
    nullable: bool,
    silent: bool,
    signature: Vec<(String, Option<usize>)>,
    locals: usize,
    body: Op
}

impl Parselet {
    // Creates a new standard parselet.
    pub fn new(body: Op, locals: usize) -> Self {
        Self{
            leftrec: false,
            nullable: true,
            silent: false,
            signature: Vec::new(),
            locals,
            body
        }
    }

    /// Creates a new silent parselet, which does always return Capture::Empty
    pub fn new_silent(body: Op, locals: usize) -> Self {
        Self{
            leftrec: false,
            nullable: true,
            silent: true,
            signature: Vec::new(),
            locals,
            body
        }
    }

    // Turn parselet into RefValue
    pub fn into_refvalue(self) -> RefValue {
        Value::Parselet(Rc::new(RefCell::new(self))).into_ref()
    }

    /** Run parselet on runtime.

    The main-parameter defines if the parselet behaves like a main loop or
    like subsequent parselet. */
    pub fn run(&self, runtime: &mut Runtime, main: bool) -> Result<Accept, Reject> {
        let mut context = Context::new(runtime, self.locals);
        let mut results = Vec::new();

        loop {
            let reader_start = context.runtime.reader.tell();
            let mut res = self.body.run(&mut context);

            /*
                In case this is the main parselet, r
            if Compiler::is_constant(&name) {hing main as much
                as possible. This will only be the case when input was
                consumed.
            */
            if main {
                //println!("main res(1) = {:?}", res);
                res = match res {
                    Ok(Accept::Next) => {
                        Ok(Accept::Repeat(None))
                    }

                    Ok(Accept::Return(value)) => {
                        Ok(Accept::Repeat(value))
                    }

                    Ok(Accept::Push(capture)) => {
                        Ok(
                            Accept::Repeat(
                                match capture {
                                    Capture::Range(range, _) => {
                                        Some(
                                            Value::String(
                                                context.runtime.reader.extract(
                                                    &range
                                                )
                                            ).into_ref()
                                        )
                                    },
                                    Capture::Value(value, _) => {
                                        Some(value)
                                    },
                                    _ => {
                                        None
                                    }
                                }
                            )
                        )
                    },
                    res => res
                };
                //println!("main res(2) = {:?}", res);
            }

            // Evaluate result of parselet loop.
            match res {
                Ok(accept) => {
                    match accept
                    {
                        Accept::Skip => {
                            return Ok(Accept::Next)
                        },

                        Accept::Return(value) => {
                            if let Some(value) = value {
                                if !self.silent {
                                    return Ok(Accept::Push(
                                        Capture::Value(value, 5)
                                    ))
                                } else {
                                    return Ok(Accept::Push(
                                        Capture::Empty
                                    ))
                                }
                            }
                            else {
                                return Ok(Accept::Push(Capture::Empty))
                            }
                        },

                        Accept::Repeat(value) => {
                            if let Some(value) = value {
                                results.push(value);
                            }
                        },

                        Accept::Push(_) if self.silent => {
                            return Ok(Accept::Push(Capture::Empty))
                        },

                        accept => return Ok(accept)
                    }

                    // In case that no more input was consumed, stop here.
                    if main && reader_start == context.runtime.reader.tell() {
                        context.runtime.reader.next();
                    }
                },

                Err(reject) => {
                    match reject {
                        Reject::Error(err) => return Err(Reject::Error(err)),
                        Reject::Main if !main => return Err(Reject::Main),
                        _ => {}
                    }

                    // Skip character
                    if main {
                        context.runtime.reader.next();
                    }
                    else if results.len() == 0 {
                        return Err(reject)
                    }
                }
            }

            if context.runtime.reader.eof() {
                break
            }
        }

        if results.len() > 1 {
            Ok(
                Accept::Push(
                    Capture::Value(
                        Value::List(Box::new(results)).into_ref(), 5
                    )
                )
            )
        }
        else if results.len() == 1 {
            Ok(
                Accept::Push(Capture::Value(results.pop().unwrap(), 5))
            )
        }
        else {
            Ok(Accept::Next)
        }
    }

    pub fn resolve(
        &mut self,
        compiler: &Compiler,
        locals: bool,
        strict: bool)
    {
        self.body.resolve(compiler, locals, strict);
    }

    pub fn finalize(statics: &Vec<RefValue>) -> usize {
        let mut changes = true;
        let mut loops = 0;

        while changes {
            changes = false;

            for i in 0..statics.len() {
                if let Value::Parselet(parselet) = &*statics[i].borrow()
                {
                    let mut parselet = parselet.borrow_mut();
                    let mut leftrec = parselet.leftrec;
                    let mut nullable = parselet.nullable;

                    parselet.body.finalize(
                        statics,
                        &mut leftrec,
                        &mut nullable
                    );

                    if !parselet.leftrec && leftrec {
                        parselet.leftrec = true;
                        changes = true;
                    }

                    if parselet.nullable && !nullable {
                        parselet.nullable = nullable;
                        changes = true;
                    }
                }
            }

            loops += 1;
        }

        println!("finalization finished after {} loops", loops);
        loops
    }
}


// --- Capture -----------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum Capture {
    Empty,                      // Empty capture
    Range(Range, u8),           // Captured range from the input & severity
    Value(RefValue, u8),        // Captured value & severity
    Named(Box<Capture>, String) // Named
}

impl Capture {
    pub fn as_value(&self, runtime: &Runtime) -> RefValue {
        match self {
            Capture::Empty => {
                Value::Void.into_ref()
            },

            Capture::Range(range, _) => {
                Value::String(
                    runtime.reader.extract(range)
                ).into_ref()
            },

            Capture::Value(value, _) => {
                value.clone()
            }

            Capture::Named(capture, _) => {
                capture.as_value(runtime)
            }
        }
    }
}


// --- Context -----------------------------------------------------------------

pub struct Context<'runtime, 'program, 'reader> {
    pub runtime: &'runtime mut Runtime<'program, 'reader>,  // fixme: Temporary pub?

    stack_start: usize,
    capture_start: usize,
    reader_start: usize
}

impl<'runtime, 'program, 'reader> Context<'runtime, 'program, 'reader> {

    pub fn new(
        runtime: &'runtime mut Runtime<'program, 'reader>,
        preserve: usize
    ) -> Self
    {
        let stack_start = runtime.stack.len();

        runtime.stack.resize(
            stack_start + preserve + 1,
            Capture::Empty
        );

        Self{
            stack_start,
            capture_start: stack_start + preserve + 1,
            reader_start: runtime.reader.tell(),
            runtime: runtime
        }
    }

    // Push value onto the stack
    pub fn push(&mut self, value: RefValue) {
        self.runtime.stack.push(Capture::Value(value, 10))
    }

    /// Pop value off the stack.
    pub fn pop(&mut self) -> RefValue {
        // todo: check for context limitations on the stack?
        let capture = self.runtime.stack.pop().unwrap();
        capture.as_value(self.runtime)
    }

    /** Return a capture by index as RefValue. */
    pub fn get_capture(&self, pos: usize) -> Option<RefValue> {
        if self.capture_start + pos >= self.runtime.stack.len() {
            return None
        }

        if pos == 0 {
            // Capture 0 either returns an already set value or ...
            if let Capture::Value(value, _) =
                &self.runtime.stack[self.capture_start]
            {
                return Some(value.clone())
            }

            // ...returns the current range read so far.
            Some(
                Value::String(
                    self.runtime.reader.extract(
                        &(self.reader_start..self.runtime.reader.tell())
                    )
                ).into_ref()
            )
        }
        else {
            Some(self.runtime.stack[
                self.capture_start + pos
            ].as_value(&self.runtime))
        }
    }

    /** Return a capture by name as RefValue. */
    pub fn get_capture_by_name(&self, name: &str) -> Option<RefValue> {
        // fixme: Should be examined in reversed order
        for capture in self.runtime.stack[self.capture_start..].iter()
        {
            if let Capture::Named(capture, alias) = &capture {
                if alias == name {
                    return Some(capture.as_value(&self.runtime))
                }
            }
        }

        None
    }

    /** Set a capture to a RefValue by index. */
    pub fn set_capture(&mut self, pos: usize, value: RefValue) {
        let pos = self.capture_start + pos;

        if pos >= self.runtime.stack.len() {
            return
        }

        self.runtime.stack[pos] = Capture::Value(value, 5)
    }

    /** Set a capture to a RefValue by name. */
    pub fn set_capture_by_name(&mut self, name: &str, value: RefValue) {
        // fixme: Should be examined in reversed order
        for capture in self.runtime.stack[self.capture_start..].iter_mut()
        {
            if let Capture::Named(capture, alias) = capture {
                if alias == name {
                    *capture = Box::new(Capture::Value(value, 5));
                    break;
                }
            }
        }
    }

    /** Get slice of all captures from current context */
    pub fn get_captures(&self) -> &[Capture] {
        &self.runtime.stack[self.capture_start..]
    }

    /** Drain all captures from current context */
    pub fn drain_captures(&mut self) -> Vec<Capture> {
        self.runtime.stack.drain(self.capture_start..).collect()
    }

    /** Helper function to collect captures from a capture_start and turn
    them either into a dict or list object capture or take them as is.

    This function is internally used for automatic AST construction and value
    inheriting.
    */
    fn collect(&mut self,
        capture_start: usize,
        copy: bool,
        single: bool) -> Option<Capture>
    {
        // Eiter copy or drain captures from stack
        let mut captures: Vec<Capture> = if copy {
            Vec::from_iter(
                self.runtime.stack[capture_start..].iter()
                    .filter(|item| !(matches!(item, Capture::Empty))).cloned()
            )
        }
        else {
            self.runtime.stack.drain(capture_start..)
                .filter(|item| !(matches!(item, Capture::Empty))).collect()
        };

        //println!("captures = {:?}", captures);

        if captures.len() == 0 {
            None
        }
        else if single && captures.len() == 1
            && !matches!(captures[0], Capture::Named(_, _)) {
            Some(captures.pop().unwrap())
        }
        else {
            let mut list = List::new();
            let mut dict = Dict::new();
            let mut max = 0;

            // Collect any significant captures and values
            for capture in captures.into_iter() {
                match capture {
                    Capture::Range(range, severity) if severity >= max => {
                        if severity > max {
                            max = severity;
                            list.clear();
                        }

                        list.push(
                            Value::String(
                                self.runtime.reader.extract(&range)
                            ).into_ref()
                        );
                    },

                    Capture::Value(value, severity) if severity >= max => {
                        if severity > max {
                            max = severity;
                            list.clear();
                        }

                        list.push(value);
                    },

                    Capture::Named(capture, alias) => {
                        // Named capture becomes dict key
                        dict.insert(alias, capture.as_value(self.runtime));
                    }

                    _ => continue
                };
            }

            //println!("list = {:?}", list);
            //println!("dict = {:?}", dict);

            if dict.len() == 0 {
                if list.len() > 1 {
                    return Some(
                        Capture::Value(
                            Value::List(Box::new(list)).into_ref(), 5
                        )
                    );
                }
                else if list.len() == 1 {
                    return Some(
                        Capture::Value(list[0].clone(), 5)
                    );
                }

                None
            }
            else {
                for (i, item) in list.into_iter().enumerate() {
                    dict.insert(i.to_string(), item);
                }

                if dict.len() == 1 {
                    return Some(
                        Capture::Value(
                            dict.values().next().unwrap().clone(), 5
                        )
                    );
                }

                Some(
                    Capture::Value(
                        Value::Dict(Box::new(dict)).into_ref(), 5
                    )
                )
            }
        }
    }
}

impl<'runtime, 'program, 'reader> Drop for Context<'runtime, 'program, 'reader> {
    fn drop(&mut self) {
        self.runtime.stack.truncate(self.stack_start);
    }
}


// --- Runtime -----------------------------------------------------------------

pub struct Runtime<'program, 'reader> {
    program: &'program Program,
    pub reader: &'reader mut Reader,  // temporary pub

    memo: HashMap<(usize, usize), (usize, Result<Accept, Reject>)>,

    stack: Vec<Capture>
}

impl<'program, 'reader> Runtime<'program, 'reader> {
    pub fn new(program: &'program Program, reader: &'reader mut Reader) -> Self {
        Self {
            program,
            reader,
            memo: HashMap::new(),
            stack: Vec::new()
        }
    }

    pub fn dump(&self) {
        println!("memo has {} entries", self.memo.len());
        println!("stack has {} entries", self.stack.len());
    }
}


// --- Program -----------------------------------------------------------------

#[derive(Debug)]
pub struct Program {
    statics: Vec<RefValue>,
    main: Rc<RefCell<Parselet>>
}

impl Program {
    pub fn new(statics: Vec<RefValue>) -> Self {
        let mut main = None;

        for i in (0..statics.len()).rev() {
            if let Value::Parselet(p) = &*statics[i].borrow() {
                main = Some(p.clone());
                break;
            }
        }

        if main.is_none() {
            panic!("No main parselet available");
        }

        Self{
            statics,
            main: main.unwrap()
        }
    }

    pub fn run(&self, runtime: &mut Runtime) -> Result<Accept, Reject> {
        let main = self.main.borrow();
        main.run(runtime, true)
    }

    pub fn run_from_str(&self, s: &'static str) -> Result<Accept, Reject> {
        let mut reader = Reader::new(Box::new(std::io::Cursor::new(s)));
        let mut runtime = Runtime::new(&self, &mut reader);

        let ret = self.run(&mut runtime);

        // tmp: report unconsumed input
        if let Some(ch) = reader.peek() {
            println!("Input was not fully consumed, next character is {:?}", ch);
        }

        ret
    }
}
