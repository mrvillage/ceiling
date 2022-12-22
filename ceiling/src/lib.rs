mod store;

pub use ceiling_macros::{group, rate_limiter};
#[cfg(feature = "async")]
pub use store::AsyncStore;
pub use store::{DefaultStore, StoreLock, SyncStore};

#[cfg(test)]
mod tests {
    use super::*;

    pub mod ceiling {
        pub use crate::store::{DefaultStore, SyncStore};
    }

    ceiling_macros::rate_limiter! {
        ip, route, method in {
            main = pub 2 requests every 2 seconds for { ip + route + method } timeout 3 seconds;
            max = 3 requests every 2 seconds for { ip + route };
        } as RateLimiter
    }

    #[test]
    fn it_works() {
        let limiter = RateLimiter::new();
        let hit_1 = limiter.hit("1.1.1.1", "/help", "GET");
        assert!(!hit_1.0);
        assert_eq!(hit_1.1.main.0, 1);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let hit_2 = limiter.hit("1.1.1.1", "/hello", "GET");
        assert!(!hit_2.0);
        assert_eq!(hit_2.1.main.0, 1);
        assert_eq!(hit_2.1.main.1, now + 2);
        limiter.hit("1.1.1.1", "/help", "GET");
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let hit_3 = limiter.hit("1.1.1.1", "/help", "GET");
        assert!(hit_3.0);
        assert_eq!(hit_3.1.main.0, 0);
        assert_eq!(hit_3.1.main.1, now + 3);
    }
}
