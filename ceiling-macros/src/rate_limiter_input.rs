use syn::{
    braced,
    parse::{Parse, ParseBuffer, ParseStream},
    Ident, Result, Token,
};

use crate::generic_input::{
    expected_arbitrary_ident, expected_duration, expected_ident, expected_ident_or_nothing,
    expected_int, expected_path, expected_token, expected_token_or_nothing,
};

pub struct RateLimiterInput {
    pub inputs: Vec<String>,
    pub rules: Vec<Rule>,
    pub name: String,
    pub store: Option<String>,
    pub async_store: bool,
}

impl Parse for RateLimiterInput {
    fn parse(mut input: ParseStream) -> Result<Self> {
        let inputs = Self::parse_inputs(&mut input)?;
        let mut body;
        braced!(body in input);
        let rules = Self::parse_body(&mut body)?;

        expected_token(&mut input, Token![as])?;
        input.parse::<Token![as]>()?;
        let name = expected_arbitrary_ident(&mut input)?;
        let async_store = expected_token_or_nothing(&mut input, Token![async]);
        if async_store {
            input.parse::<Token![async]>()?;
        }
        let store = if expected_token_or_nothing(&mut input, Token![in]) {
            input.parse::<Token![in]>()?;
            Some(expected_path(&mut input)?)
        } else {
            None
        };
        Ok(RateLimiterInput {
            inputs,
            rules,
            name,
            store,
            async_store,
        })
    }
}

impl RateLimiterInput {
    fn parse_inputs(input: &mut ParseStream) -> Result<Vec<String>> {
        let mut inputs = Vec::new();
        loop {
            let lookahead = input.lookahead1();
            if lookahead.peek(Token![in]) {
                input.parse::<Token![in]>()?;
                break;
            } else if lookahead.peek(Token![,]) {
                input.parse::<Token![,]>()?;
            } else if lookahead.peek(Ident) {
                let ident = input.parse::<Ident>()?;
                inputs.push(ident.to_string());
            } else {
                lookahead.error();
            }
        }
        Ok(inputs)
    }

    fn parse_body(input: &mut ParseBuffer) -> Result<Vec<Rule>> {
        Ok(input
            .parse_terminated::<_, Token![;]>(Rule::parse)?
            .into_iter()
            .collect::<Vec<_>>())
    }
}

#[derive(Debug)]
pub struct Rule {
    pub name: String,
    pub limit: u32,
    pub interval: u32,
    pub timeout: u32,
    pub key: Vec<String>,
    pub public: bool,
}

impl Parse for Rule {
    fn parse(mut input: ParseStream) -> Result<Self> {
        let name = expected_arbitrary_ident(&mut input)?;
        expected_token(&mut input, Token![=])?;
        input.parse::<Token![=]>()?;

        let public = if input.peek(Token![pub]) {
            input.parse::<Token![pub]>()?;
            true
        } else {
            false
        };

        let limit = expected_int(&mut input)?;
        expected_ident(&mut input, "requests")?;
        expected_ident(&mut input, "every")?;
        let interval = expected_duration(&mut input)?;
        expected_token(&mut input, Token![for])?;
        input.parse::<Token![for]>()?;
        let key;
        braced!(key in input);
        let key = Self::parse_key(key)?;
        let timeout = expected_ident_or_nothing(&mut input, "timeout")?;
        let timeout = if timeout {
            expected_duration(&mut input)?
        } else {
            interval
        };
        Ok(Rule {
            name,
            limit,
            interval,
            timeout,
            key,
            public,
        })
    }
}

impl Rule {
    fn parse_key(input: ParseBuffer) -> Result<Vec<String>> {
        Ok(input
            .parse_terminated::<_, Token![+]>(|buf| {
                let lookahead = buf.lookahead1();
                if lookahead.peek(Ident) {
                    Ok(buf.parse::<Ident>()?.to_string())
                } else {
                    Err(lookahead.error())
                }
            })?
            .into_iter()
            .collect::<Vec<_>>())
    }
}
