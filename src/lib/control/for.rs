use crate::lang::argument::Argument;
use crate::lang::command::Command;
use crate::lang::errors::{argument_error, CrushResult, mandate};
use crate::lang::execution_context::{ArgumentVector, ExecutionContext};
use crate::lang::stream::{black_hole, empty_channel, CrushStream};
use crate::lang::value::Value;
use crate::lang::{dict::DictReader, list::ListReader, r#struct::Struct, table::TableReader};
use crate::lang::r#struct::StructReader;

pub fn r#for(mut context: ExecutionContext) -> CrushResult<()> {
    context.output.send(Value::Empty());
    context.arguments.check_len(2)?;

    let body = context.arguments.command(1)?;
    let iter = context.arguments.remove(0);
    let name = iter.argument_type;
    let mut input = mandate(iter.value.stream(), "Expected a stream")?;

    while let Ok(line) = input.read() {
        let env = context.env.create_child(&context.env, true);
        let arguments = match &name {
            None => line
                .into_vec()
                .drain(..)
                .zip(input.types().iter())
                .map(|(c, t)| Argument::named(&t.name, c))
                .collect(),
            Some(var_name) => {
                if input.types().len() == 1 {
                    vec![Argument::new(
                        Some(var_name.clone()),
                        line.into_vec().remove(0),
                    )]
                } else {
                    vec![Argument::new(
                        Some(var_name.clone()),
                        Value::Struct(Struct::from_vec(line.into_vec(), input.types().to_vec())),
                    )]
                }
            }
        };
        body.invoke(ExecutionContext {
            input: empty_channel(),
            output: black_hole(),
            arguments,
            env: env.clone(),
            this: None,
            printer: context.printer.clone(),
        })?;
        if
        env.
            is_stopped() {
            break;
        }
    }
    Ok(())
}
