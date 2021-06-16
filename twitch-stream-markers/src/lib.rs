use anyhow::Context;
use hyper::{
    body::{aggregate, Buf},
    client::HttpConnector,
    header::AUTHORIZATION,
    Body, Request,
};
use hyper_rustls::HttpsConnector;
use serde::{Deserialize, Serialize};
use std::future::Future;

#[derive(Serialize)]
struct CreateMarker<'a> {
    user_id: &'a str,
    description: Option<&'a str>,
}

#[derive(Debug, Deserialize)]
struct Response<T> {
    data: Vec<T>,
}

#[derive(Debug, Deserialize)]
struct User {
    id: String,
}

#[derive(Debug, Deserialize)]
pub struct Marker {
    pub id: String,
    pub created_at: String,
    pub description: String,
    pub position_seconds: i32,
}

pub struct Client {
    client: hyper::Client<HttpsConnector<HttpConnector>>,
    user_id: String,
    auth: String,
}

impl Client {
    pub async fn new(token: &str) -> anyhow::Result<Self> {
        let auth = format!("Bearer {}", token);
        let https = HttpsConnector::with_native_roots();
        let client = hyper::Client::builder().build(https);

        let response = client
            .request(
                Request::get("https://api.twitch.tv/helix/users")
                    .header(AUTHORIZATION, auth.as_str())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await?;

        let bytes = aggregate(response.into_body()).await?;
        let users: Response<User> = serde_json::from_reader(bytes.reader())?;

        Ok(Self {
            client,
            user_id: users
                .data
                .into_iter()
                .next()
                .context("Twitch didn't respond with a User ID.")?
                .id,
            auth,
        })
    }

    pub fn create_marker(
        &self,
        description: Option<&str>,
    ) -> impl Future<Output = anyhow::Result<Marker>> {
        let request = self.client.request(
            Request::post("https://api.twitch.tv/helix/streams/markers")
                .header(AUTHORIZATION, self.auth.as_str())
                .body(
                    serde_json::to_vec(&CreateMarker {
                        user_id: &self.user_id,
                        description,
                    })
                    .unwrap()
                    .into(),
                )
                .unwrap(),
        );

        async move {
            let bytes = aggregate(request.await?.into_body()).await?;
            let markers: Response<Marker> = serde_json::from_reader(bytes.reader())?;

            Ok(markers
                .data
                .into_iter()
                .next()
                .context("Twitch didn't respond with a marker.")?)
        }
    }
}
