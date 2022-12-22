//! Ceiling is a simple, lightweight, and highly configurable library for handling and creating rate limiting rules.
//!
//! The main entrypoint to the library is the `rate_limiter!` macro found below.
mod generic_input;
mod group_input;
mod rate_limiter_input;

use group_input::GroupInput;
use proc_macro2::TokenStream;
use quote::quote;
use rand::distributions::DistString;
use rate_limiter_input::{RateLimiterInput, Rule};
use syn::{parse_macro_input, Ident, LitStr, Path, Result};

/// This macro is the entrypoint for creating rate limiting rules with ceiling.
/// The macro takes input corresponding to the inputs to the rate limiter and the rules.
///
/// # Example
/// ```
/// ceiling::rate_limiter! {
///     // takes in three inputs named `ip`, `route`, and `method`
///     // they must implement `std::fmt::Display` so they can be coerced into strings as needed
///     ip, route, method in {
///         // the following creates a public (detailed information is meant to be returned to the client) rate limiting rule named main with a limit of 2 requests every 2 seconds (interval) for the key created by concatenating the ip, route, and method inputs together
///         // when the rate limit is hit, the timeout specified is 3 seconds from the time of the request that emptied the bucket
///         main = pub 2 requests every 2 seconds for { ip + route + method } timeout 3 seconds;
///         // the following only contains the required components of a rate limiting rule
///         // this one crates a private rate limiting rule with a limit of 3 request every 2 minutes (interval) for the key ip + route
///         // since timeout is not specified, the bucket will reset when the interval is up
///         burst = 3 requests every 2 minutes for { ip + route };
///     // `as RateLimiter` tells the macro to name the generated struct RateLimiter
///     // `async` says the following custom store is asynchronous
///     // i.e. implements `ceiling::AsyncStore` instead of `ceiling::SyncStore`
///     // `in crate::MyAsyncStore` tells the macro to use the struct `crate::MyAsyncStore` for the bucket stores
///     } as RateLimiter async in crate::MyAsyncStore
/// }
/// ```
/// ```
/// let rate_limiter = RateLimiter::new();
/// // "hits" the rate limiter, what would happen when someone, for example, makes a request
/// // the return result is a `bool` (`rate_limiter`) of whether the request is being rate limiter (`true` means it is and should not continue)
/// // and a `RateLimiterHit` (the name of the struct is rate limiter name + "Hit") struct containing detailed metadata on the state of all the rate limiting rules
/// // rules can be found by using the name of the rule, i.e. `hit.main` corresponds to the rule named `main`
/// // the value of a rule's metadata is a tuple of type `(u32, u64, bool, String)` corresponding to the requests remaining, the reset time, whether the rule is public or not, and the key of the bucket
/// let (rate_limiter, hit) = rate_limiter.hit("1.1.1.1", "/example", "GET").await;
/// // with the crate feature `serde` enabled, the `hit` object implements `serde::Serialize` and can be easily serialized to any format
/// // the serialized data will only contain the public rules, the various fields can be found below
/// // as another option, the hit object has a `to_headers` method that will return a Vec<(&str, String)> corresponding to the header and value
/// // information on the headers can be found below
/// let headers = hit.to_headers();
/// for (header, value) in headers {
///     response.header(header, value);
/// }
/// ```
///
/// ## Headers/Metadata Attributes
/// | Header                  | Attribute     | Description                                                                                     |
/// | ----------------------- | ------------- | ----------------------------------------------------------------------------------------------- |
/// | X-RateLimit-Limit       | "limit"       | limit of hits per interval seconds                                                              |
/// | X-RateLimit-Interval    | "interval"    | interval before bucket resets after first hit                                                   |
/// | X-RateLimit-Timeout     | "timeout"     | timeout before the bucket resets after limit is reached                                         |
/// | X-RateLimit-Remaining   | "remaining"   | hits remaining in interval                                                                      |
/// | X-RateLimit-Reset       | "reset"       | timestamp in seconds when the bucket resets                                                     |
/// | X-RateLimit-Reset-After | "reset_after" | seconds until bucket resets                                                                     |
/// | X-RateLimit-Key         | "key"         | the bucket key, may be shared between routes and therefore useful for client-side rate limiting |
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
    let use_store = if async_store {
        quote!(
            use ceiling::AsyncStore;
        )
    } else {
        quote!(
            use ceiling::SyncStore;
        )
    };
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
                    #use_store

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
    let prune = if async_store {
        quote!(self.#name.prune(now).await)
    } else {
        quote!(self.#name.prune(now))
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
            #prune;
            (#name.0, #name.1, #public, key)
        };
    }
}

/// `group!` is a utility macro for grouping multiple values into a single key
///
/// # Example
/// ```
/// // this will generate a function called `bucket` that takes an &str and returns an &str
/// // if the provided value matches any of the values in the macro it will return a shared bucket key
/// // i.e. `bucket("/help")` will return the same value as `bucket("/help2")`
/// // if no matches are found, then it will return the value provided
/// ceiling::group! {
///     bucket {
///         "/help", "/help2", "/help3";
///         "/one", "/two";
///     }
/// }
/// ```
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
