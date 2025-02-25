//! Intermediate representation of a parselet

use super::*;
use crate::value::Parselet;

#[derive(Debug)]
pub struct ImlParselet {
    pub consuming: Option<Consumable>,           // Consumable state
    pub severity: u8,                            // Capture push severity
    pub name: Option<String>,                    // Parselet's name from source (for debugging)
    pub signature: Vec<(String, Option<usize>)>, // Argument signature with default arguments
    locals: usize,                               // Number of local variables present
    begin: ImlOp,                                // Begin-operations
    end: ImlOp,                                  // End-operations
    body: ImlOp,                                 // Operations
}

impl ImlParselet {
    /// Creates a new intermediate parselet.
    pub fn new(
        name: Option<String>,
        signature: Vec<(String, Option<usize>)>,
        locals: usize,
        begin: ImlOp,
        end: ImlOp,
        body: ImlOp,
    ) -> Self {
        assert!(
            signature.len() <= locals,
            "signature may not be longer than locals..."
        );

        Self {
            name,
            consuming: None,
            severity: 5,
            signature,
            locals,
            begin,
            end,
            body,
        }
    }

    // Turns an ImlParselet in to a parselet
    pub fn into_parselet(&self /* fixme: change to self without & later on... */) -> Parselet {
        Parselet::new(
            self.name.clone(),
            if let Some(Consumable { leftrec, .. }) = self.consuming {
                Some(leftrec)
            } else {
                None
            },
            self.severity,
            self.signature.clone(),
            self.locals,
            self.begin.compile(&self),
            self.end.compile(&self),
            self.body.compile(&self),
        )
    }

    pub fn resolve(&mut self, usages: &mut Vec<Vec<ImlOp>>) {
        self.begin.resolve(usages);
        self.end.resolve(usages);
        self.body.resolve(usages);
    }

    pub fn finalize(
        &mut self,
        values: &Vec<ImlValue>,
        stack: &mut Vec<(usize, bool)>,
    ) -> Option<Consumable> {
        self.body.finalize(values, stack)
    }
}

impl std::cmp::PartialEq for ImlParselet {
    // It satisfies to just compare the parselet's memory address for equality
    fn eq(&self, other: &Self) -> bool {
        self as *const ImlParselet as usize == other as *const ImlParselet as usize
    }
}

impl std::hash::Hash for ImlParselet {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        (self as *const ImlParselet as usize).hash(state);
    }
}

impl std::cmp::PartialOrd for ImlParselet {
    // It satisfies to just compare the parselet's memory address for equality
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        let left = self as *const ImlParselet as usize;
        let right = other as *const ImlParselet as usize;

        left.partial_cmp(&right)
    }
}
