use syn::{
    braced,
    parse::{Parse, ParseStream},
    LitStr, Result, Token,
};

use crate::generic_input::expected_arbitrary_ident;

pub struct GroupInput {
    pub name: String,
    pub groups: Vec<Vec<String>>,
}

impl Parse for GroupInput {
    fn parse(mut input: ParseStream) -> Result<Self> {
        let name = expected_arbitrary_ident(&mut input)?;
        let stream;
        braced!(stream in input);
        let mut groups = vec![];
        let mut current = vec![];
        loop {
            if stream.is_empty() {
                break;
            }
            let lookahead = stream.lookahead1();
            if lookahead.peek(Token![,]) {
                stream.parse::<Token![,]>()?;
                continue;
            } else if lookahead.peek(Token![;]) {
                groups.push(current);
                current = vec![];
                stream.parse::<Token![;]>()?;
            } else if lookahead.peek(LitStr) {
                current.push(stream.parse::<LitStr>()?.value());
            } else {
                return Err(lookahead.error());
            }
        }
        Ok(Self { name, groups })
    }
}
