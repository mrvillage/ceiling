mod generic_input;
mod group_input;
mod rate_limiter_input;

use group_input::GroupInput;
use proc_macro2::TokenStream;
use quote::quote;
use rand::distributions::DistString;
use rate_limiter_input::{RateLimiterInput, Rule};
use syn::{parse_macro_input, Ident, LitStr, Path, Result};

#[proc_macro]
pub fn rate_limiter(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    impl_rate_limiter(parse_macro_input!(input as RateLimiterInput))
        .unwrap()
        .into()
}

fn impl_rate_limiter(
    RateLimiterInput {
        inputs,
        rules,
        name,
        store,
        async_store,
    }: RateLimiterInput,
) -> Result<TokenStream> {
    let name = syn::parse_str::<syn::Ident>(&name)?;
    let store = syn::parse_str::<Path>(&store.unwrap_or_else(|| "ceiling::DefaultStore".into()))?;

    let input_type_params = inputs
        .iter()
        .map(|i| syn::parse_str::<syn::Ident>(format!("{}_IN", i.to_uppercase()).as_str()).unwrap())
        .collect::<Vec<_>>();
    let inputs = inputs
        .iter()
        .map(|i| syn::parse_str::<syn::Ident>(format!("{i}_input").as_str()).unwrap())
        .collect::<Vec<_>>();
    let input_params = inputs
        .iter()
        .zip(&input_type_params)
        .map(|(i, t)| quote!(#i: #t))
        .collect::<Vec<_>>();

    let hit = syn::parse_str::<syn::Ident>(format!("{}Hit", name).as_str())?;

    let rule_names = rules
        .iter()
        .map(|r| syn::parse_str::<syn::Ident>(&r.name).unwrap())
        .collect::<Vec<_>>();
    let rule_impls = rules
        .iter()
        .map(|r| impl_rule(r, async_store))
        .collect::<Vec<_>>();

    let num_rules = rules.iter().filter(|r| r.public).count();
    let num_headers = num_rules * 7;

    let rules_serde = rule_names.iter().zip(&rules).map(|(name, r)| {
        let Rule {
            name: _,
            limit,
            interval,
            timeout,
            key: _,
            public,
        } = r;
        if *public {
            quote! {
                let mut m: std::collections::HashMap<&str, Val> = std::collections::HashMap::with_capacity(7);
                m.insert("limit", #limit.into());
                m.insert("interval", #interval.into());
                m.insert("timeout", #timeout.into());
                m.insert("remaining", self.#name.0.into());
                m.insert("reset", self.#name.1.into());
                m.insert("reset_after", (self.#name.1).saturating_sub(now).into());
                m.insert("key", (&self.#name.3).into());
                map.serialize_entry(stringify!(self.#name), &m)?;
            }
        } else {
            quote!()
        }
    });
    let rules_headers = rule_names.iter().zip(&rules).map(|(name, r)| {
        let Rule {
            name: _,
            limit,
            interval,
            timeout,
            key: _,
            public,
        } = r;
        if *public {
            quote! {
                vec.push(("X-RateLimit-Limit", format!("{} {}", stringify!(#name), #limit)));
                vec.push(("X-RateLimit-Interval", format!("{} {}", stringify!(#name), #interval)));
                vec.push(("X-RateLimit-Timeout", format!("{} {}", stringify!(#name), #timeout)));
                vec.push(("X-RateLimit-Remaining", format!("{} {}", stringify!(#name), self.#name.0)));
                vec.push(("X-RateLimit-Reset", format!("{} {}", stringify!(#name), self.#name.1)));
                vec.push(("X-RateLimit-Reset-After", format!("{} {}", stringify!(#name), (self.#name.1).saturating_sub(now))));
                vec.push(("X-RateLimit-Key", format!("{} {}", stringify!(#name), self.#name.3)));
            }
        } else {
            quote!()
        }
    });

    let async_hit = if async_store { quote!(async) } else { quote!() };
    Ok(quote! {
        #[derive(Clone, Debug)]
        pub struct #name {
            #(#rule_names: std::sync::Arc<#store>),*
        }

        impl #name {
            pub fn new() -> Self {
                Self {
                    #(#rule_names: std::sync::Arc::new(#store::new())),*
                }
            }

            pub #async_hit fn hit<#(#input_type_params),*>(&self, #(#input_params),*) -> (bool, #hit)
            where
                #(#input_type_params: std::fmt::Display),*
                {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs();
                    let mut hit = false;
                    #(#rule_impls)*
                    (hit, #hit {
                        #(#rule_names),*
                    })
                }
        }

        #[derive(Clone, Debug)]
        pub struct #hit {
            pub #(#rule_names: (u32, u64, bool, String)),*
        }

        impl #hit {
            pub fn to_headers(&self) -> Vec<(&str, String)> {
                let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
                let mut vec = Vec::with_capacity(#num_headers);
                #(#rules_headers)*
                vec
            }
        }

        #[cfg(feature = "serde")]
        impl serde::Serialize for #hit {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                use serde::ser::SerializeMap;

                let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
                let mut map = serializer.serialize_map(Some(#num_rules))?;
                #(#rules_serde)*
                map.end()
            }
        }

        #[cfg(feature = "serde")]
        enum Val {
            Int(u64),
            Str(String),
        }

        #[cfg(feature = "serde")]
        impl From<u32> for Val {
            fn from(v: u32) -> Val {
                Val::Int(v as u64)
            }
        }

        #[cfg(feature = "serde")]
        impl From<u64> for Val {
            fn from(v: u64) -> Val {
                Val::Int(v)
            }
        }

        #[cfg(feature = "serde")]
        impl From<&String> for Val {
            fn from(v: &String) -> Val {
                Val::Str(v.to_string())
            }
        }

        #[cfg(feature = "serde")]
        impl serde::Serialize for Val {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                match self {
                    Self::Int(v) => serializer.serialize_u64(*v),
                    Self::Str(v) => serializer.serialize_str(v),
                }
            }
        }

    })
}

fn impl_rule(rule: &Rule, async_store: bool) -> TokenStream {
    let Rule {
        name,
        limit,
        interval,
        timeout,
        key,
        public,
    } = rule;
    let name = syn::parse_str::<syn::Ident>(name).unwrap();
    let key = key
        .iter()
        .map(|k| syn::parse_str::<syn::Ident>(format!("{k}_input").as_str()).unwrap())
        .collect::<Vec<_>>();
    let key = if key.is_empty() {
        quote!("".to_string())
    } else {
        let lit = key.iter().map(|_| "{}").collect::<Vec<_>>().join("+");
        quote!(format!(#lit, #(#key),*))
    };
    let get = if async_store {
        quote!(self.#name.get(&key).await)
    } else {
        quote!(self.#name.get(&key))
    };
    let set = if async_store {
        quote!(self.#name.set(&key, #name, reset_updated).await)
    } else {
        quote!(self.#name.set(&key, #name, reset_updated))
    };
    quote! {
        let #name = {
            let key = #key;
            let lock = #get;
            let mut #name = (*lock).unwrap_or((#limit, now + (#interval as u64)));
            let mut reset_updated = false;
            if #name.1 < now {
                #name = (#limit, now + (#interval as u64));
                reset_updated = true;
            }
            if #name.0 > 1 {
                #name.0 -= 1;
                #set;
            } else if #name.0 == 1 {
                #name = (0, now + (#timeout as u64));
                reset_updated = true;
                #set;
                hit = true;
            } else {
                hit = true;
            }
            drop(lock);
            (#name.0, #name.1, #public, key)
        };
    }
}

#[proc_macro]
pub fn group(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    impl_group(parse_macro_input!(input as GroupInput))
        .unwrap()
        .into()
}

fn impl_group(GroupInput { name, groups }: GroupInput) -> Result<TokenStream> {
    let groups = groups.into_iter().enumerate().map(|(i, g)| {
        let s = syn::parse_str::<LitStr>(
            format!(
                "\"__{}-{}\"",
                i,
                rand::distributions::Alphanumeric.sample_string(&mut rand::thread_rng(), 20),
            )
            .as_str(),
        )
        .unwrap();
        quote! {
            #(
                #g => #s,
            )*
        }
    });
    let name = syn::parse_str::<Ident>(&name)?;
    let gen = quote! {
        fn #name(value: &str) -> &str {
            match value {
                #( #groups )*
                _ => value
            }
        }
    };
    Ok(gen)
}
