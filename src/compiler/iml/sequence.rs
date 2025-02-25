use super::*;

/** Sequence construct.

This intermediate language construct collects a sequence of operations or sequence of further
constructs.

According to these operation's semantics, or when an entire sequence is completely recognized,
the sequence is getting accepted. Incomplete sequences are rejected, but might partly be
processed, including data changes, which is a wanted behavior.
*/

#[derive(Debug)]
pub struct ImlSequence {
    consuming: Option<Consumable>, // Consumable state
    items: Vec<ImlOp>,
}

impl ImlSequence {
    pub fn new(items: Vec<ImlOp>) -> ImlOp {
        Self {
            consuming: None,
            items,
        }
        .into_op()
    }
}

impl Compileable for ImlSequence {
    fn resolve(&mut self, usages: &mut Vec<Vec<ImlOp>>) {
        /*
            Sequences are *the* special case for symbol resolving.
            When a resolve replaces one Op by multiple Ops, and this
            happens inside of a sequence, then the entire sequence
            must be extended in-place.

            So `a B c D e` may become `a x c y z e`.

            This could probably be made more fantastic with ac real
            VM concept, but I'm just happy with this right now.
        */
        let mut end = self.items.len();
        let mut i = 0;

        while i < end {
            let item = self.items.get_mut(i).unwrap();

            if let ImlOp::Usage(usage) = *item {
                let n = usages[usage].len();

                self.items.splice(i..i + 1, usages[usage].drain(..));

                i += n;
                end = self.items.len();
            } else {
                i += 1
            }
        }

        for item in self.items.iter_mut() {
            item.resolve(usages);
        }
    }

    fn finalize(
        &mut self,
        values: &Vec<ImlValue>,
        stack: &mut Vec<(usize, bool)>,
    ) -> Option<Consumable> {
        let mut leftrec = false;
        let mut nullable = true;
        let mut consumes = false;

        for item in self.items.iter_mut() {
            if !nullable {
                break;
            }

            if let Some(consumable) = item.finalize(values, stack) {
                leftrec |= consumable.leftrec;
                nullable = consumable.nullable;
                consumes = true;
            }
        }

        // Hold meta information about consuming state.
        if stack.len() == 1 && consumes {
            self.consuming = Some(Consumable { leftrec, nullable });
        }

        if consumes {
            Some(Consumable { leftrec, nullable })
        } else {
            None
        }
    }

    fn compile(&self, parselet: &ImlParselet) -> Vec<Op> {
        let mut ret = Vec::new();

        for item in self.items.iter() {
            ret.extend(item.compile(parselet));
        }

        if ret.len() > 1 {
            ret.insert(0, Op::Frame(0));
            ret.push(Op::Collect(0));
            ret.push(Op::Close);
        }

        ret
    }
}

impl std::fmt::Display for ImlSequence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "(")?;
        for item in &self.items {
            write!(f, "{} ", item)?;
        }
        write!(f, ")")?;

        Ok(())
    }
}
