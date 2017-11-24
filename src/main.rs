extern crate chrono;
extern crate dissolve;
extern crate egg_mode;
extern crate mammut;
extern crate regex;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;
extern crate tokio_core;
extern crate toml;

use egg_mode::tweet::DraftTweet;
use mammut::Mastodon;
use mammut::status_builder::StatusBuilder;
use std::fs::File;
use std::io::prelude::*;
use tokio_core::reactor::Core;

use config::*;
use registration::mastodon_register;
use registration::twitter_register;
use sync::determine_posts;
use delete_statuses::mastodon_delete_older_statuses;
use delete_statuses::twitter_delete_older_statuses;

mod config;
mod registration;
mod sync;
mod delete_statuses;

fn main() {
    let config = match File::open("mastodon-twitter-sync.toml") {
        Ok(f) => config_load(f),
        Err(_) => {
            let mastodon = mastodon_register();
            let twitter_config = twitter_register();
            let config = Config {
                mastodon: MastodonConfig {
                    app: (*mastodon).clone(),
                    // Do not delete older status per default, users should
                    // enable this explicitly.
                    delete_older_statuses: false,
                },
                twitter: twitter_config,
            };

            // Save config for using on the next run.
            let toml = toml::to_string(&config).unwrap();
            let mut file = File::create("mastodon-twitter-snyc.toml").unwrap();
            file.write_all(toml.as_bytes()).unwrap();

            config
        }
    };

    let mastodon = Mastodon::from_data(config.mastodon.app);

    let account = mastodon.verify().unwrap();
    let mastodon_statuses = mastodon
        .statuses(account.id, false, true, None, None)
        .unwrap();

    let con_token =
        egg_mode::KeyPair::new(config.twitter.consumer_key, config.twitter.consumer_secret);
    let access_token = egg_mode::KeyPair::new(
        config.twitter.access_token,
        config.twitter.access_token_secret,
    );
    let token = egg_mode::Token::Access {
        consumer: con_token,
        access: access_token,
    };

    let mut core = Core::new().unwrap();
    let handle = core.handle();
    let mut timeline =
        egg_mode::tweet::user_timeline(config.twitter.user_id, false, true, &token, &handle)
            .with_page_size(50);

    let tweets = core.run(timeline.start()).unwrap();
    let posts = determine_posts(&mastodon_statuses, &*tweets);

    for toot in posts.toots {
        println!("Posting to Mastodon: {}", toot);
        mastodon.new_status(StatusBuilder::new(toot)).unwrap();
    }

    for tweet in posts.tweets {
        println!("Posting to Twitter: {}", tweet);
        core.run(DraftTweet::new(&tweet).send(&token, &handle))
            .unwrap();
    }

    // Delete old mastodon statuses if that option is enabled.
    if config.mastodon.delete_older_statuses {
        mastodon_delete_older_statuses(&mastodon, &account);
    }
    if config.twitter.delete_older_statuses {
        twitter_delete_older_statuses(config.twitter.user_id, &token);
    }
}
