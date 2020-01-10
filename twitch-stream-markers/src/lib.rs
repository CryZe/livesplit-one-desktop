use {
    bytes::buf::BufExt,
    hyper::{body::aggregate, client::HttpConnector, header::AUTHORIZATION, Body, Request},
    hyper_rustls::HttpsConnector,
    serde::{Deserialize, Serialize},
    std::future::Future,
};

// pub use futures;

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
    pub async fn new(token: impl AsRef<str>) -> anyhow::Result<Self> {
        let auth = format!("Bearer {}", token.as_ref());
        let https = HttpsConnector::new();
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
        let mut users: Response<User> = serde_json::from_reader(bytes.reader())?;

        Ok(Self {
            client,
            user_id: users.data.remove(0).id,
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
                        description: description.into(),
                    })
                    .unwrap()
                    .into(),
                )
                .unwrap(),
        );

        async move {
            let bytes = aggregate(request.await?.into_body()).await?;
            let mut markers: Response<Marker> = serde_json::from_reader(bytes.reader())?;

            Ok(markers.data.remove(0))
        }
    }
}
