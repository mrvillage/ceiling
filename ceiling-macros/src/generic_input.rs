use syn::{parse::ParseStream, parse::Peek, Ident, LitInt, Result, Token};

pub fn expected_ident(input: &mut ParseStream, ident: &str) -> Result<()> {
    let lookahead = input.lookahead1();
    if lookahead.peek(Ident) {
        let i = input.parse::<Ident>()?;
        if i != ident {
            return Err(syn::Error::new(i.span(), format!("expected '{}'", ident)));
        }
        Ok(())
    } else {
        Err(lookahead.error())
    }
}

pub fn expected_ident_or_nothing(input: &mut ParseStream, ident: &str) -> Result<bool> {
    let lookahead = input.lookahead1();
    if lookahead.peek(Ident) {
        let i = input.parse::<Ident>()?;
        if i != ident {
            return Err(syn::Error::new(i.span(), format!("expected '{}'", ident)));
        }
        Ok(true)
    } else {
        Ok(false)
    }
}

pub fn expected_int(input: &mut ParseStream) -> Result<u32> {
    let lookahead = input.lookahead1();
    if lookahead.peek(LitInt) {
        Ok(input.parse::<LitInt>()?.base10_parse::<u32>()?)
    } else {
        Err(lookahead.error())
    }
}

pub fn expected_duration(input: &mut ParseStream) -> Result<u32> {
    let duration = expected_int(input)?;
    let lookahead = input.lookahead1();
    let duration = if lookahead.peek(Ident) {
        let ident = input.parse::<Ident>()?;
        match ident.to_string().as_str() {
            "second" | "seconds" => duration,
            "minute" | "minutes" => duration * 60,
            "hour" | "hours" => duration * 60 * 60,
            "day" | "days" => duration * 60 * 60 * 24,
            _ => {
                return Err(syn::Error::new(
                    ident.span(),
                    "expected 'seconds', 'minutes', 'hours', or 'days'",
                ))
            },
        }
    } else {
        return Err(lookahead.error());
    };
    Ok(duration)
}

pub fn expected_token<T: Peek>(input: &mut ParseStream, token: T) -> Result<()> {
    let lookahead = input.lookahead1();
    if lookahead.peek(token) {
        Ok(())
    } else {
        Err(lookahead.error())
    }
}

pub fn expected_token_or_nothing<T: Peek>(input: &mut ParseStream, token: T) -> bool {
    input.peek(token)
}

pub fn expected_arbitrary_ident(input: &mut ParseStream) -> Result<String> {
    let lookahead = input.lookahead1();
    if lookahead.peek(Ident) {
        Ok(input.parse::<Ident>()?.to_string())
    } else {
        Err(lookahead.error())
    }
}

pub fn expected_path(input: &mut ParseStream) -> Result<String> {
    let mut path = String::new();
    loop {
        let lookahead = input.lookahead1();
        if lookahead.peek(Token![::]) {
            path.push_str("::");
            input.parse::<Token![::]>()?;
        } else if lookahead.peek(Ident) {
            path.push_str(&input.parse::<Ident>()?.to_string());
        } else {
            break;
        }
    }
    if path.is_empty() {
        return Err(syn::Error::new(input.span(), "expected path"));
    }
    Ok(path)
}
