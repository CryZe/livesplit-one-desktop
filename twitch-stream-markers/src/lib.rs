use {
    futures::{sync::oneshot, Future, Stream},
    hyper::{
        client::HttpConnector, header::AUTHORIZATION, service::service_fn_ok, Body, Request, Server,
    },
    // reqwest::{header::AUTHORIZATION, r#async::Client as HttpClient},
    hyper_rustls::HttpsConnector,
    serde::{Deserialize, Serialize},
    std::sync::{Arc, Mutex},
    url::Url,
};

pub use futures;

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
    pub fn new(token: impl AsRef<str>) -> impl Future<Item = Self, Error = ()> {
        let auth = format!("Bearer {}", token.as_ref());
        let https = HttpsConnector::new(4);
        let client = hyper::Client::builder().build(https);

        client
            .request(
                Request::get("https://api.twitch.tv/helix/users")
                    .header(AUTHORIZATION, auth.as_str())
                    .body(Body::empty())
                    .unwrap(),
            )
            .map_err(drop)
            .and_then(|response| response.into_body().concat2().map_err(drop))
            .and_then(|body| serde_json::from_slice(&body).map_err(drop))
            .map(|mut users: Response<User>| Self {
                client,
                user_id: users.data.remove(0).id,
                auth,
            })
    }

    pub fn create_marker(
        &self,
        description: Option<&str>,
    ) -> impl Future<Item = Marker, Error = ()> {
        self.client
            .request(
                Request::post("https://api.twitch.tv/helix/streams/markers")
                    .header(AUTHORIZATION, self.auth.as_str())
                    .body(
                        serde_json::to_vec(&CreateMarker {
                            user_id: &self.user_id,
                            description: description.into(),
                        })
                        .unwrap()
                        .into(),
                    )
                    .unwrap(),
            )
            .map_err(drop)
            // .and_then(|response| response.error_for_status())
            .and_then(|response| response.into_body().concat2().map_err(drop))
            .and_then(|body| serde_json::from_slice(&body).map_err(drop))
            .map(|mut markers: Response<Marker>| markers.data.remove(0))
    }

    // pub fn get_markers(&self) -> impl Future<Item = Vec<Marker>, Error = Error> {
    //     self.client
    //         .get(
    //             Url::parse_with_params(
    //                 "https://api.twitch.tv/helix/streams/markers",
    //                 &[("user_id", &self.user_id)],
    //             )
    //             .unwrap(),
    //         )
    //         .send()
    //         .and_then(|response| response.error_for_status())
    //         .and_then(|mut response| response.json())
    //         .map(|markers: Response<Marker>| markers.data)
    // }
}
