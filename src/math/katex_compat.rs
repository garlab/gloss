//! Compatibility with the square-root builder in katex-rs 0.3.0.

use std::sync::{Arc, OnceLock};

use katex_native::katex::{
    KatexContext,
    define_function::{FunctionContext, FunctionHandler},
    parser::parse_node::{ParseNode, ParseNodeOrdGroup},
    types::ParseError,
};

static SQRT_HANDLER: OnceLock<FunctionHandler> = OnceLock::new();

pub(super) fn context() -> KatexContext {
    let mut context = KatexContext::default();
    let sqrt = Arc::make_mut(context.functions.get_mut("\\sqrt").expect("sqrt function"));
    SQRT_HANDLER.get_or_init(|| sqrt.handler.expect("sqrt parser"));
    sqrt.handler = Some(group_sqrt_argument);
    context
}

fn group_sqrt_argument(
    context: FunctionContext<'_, '_, '_>,
    mut args: Vec<ParseNode>,
    optional_args: Vec<Option<ParseNode>>,
) -> Result<ParseNode, ParseError> {
    // The HTML builder puts the radical's left padding on a DomSpan. Group a
    // bare argument after macro expansion so it gets the same span as `{x}`.
    if !matches!(args[0], ParseNode::OrdGroup(_)) {
        let body = args.remove(0);
        args.push(ParseNode::OrdGroup(ParseNodeOrdGroup {
            mode: body.mode(),
            loc: None,
            body: vec![body],
            semisimple: None,
        }));
    }
    SQRT_HANDLER.get().expect("sqrt parser initialized")(context, args, optional_args)
}
