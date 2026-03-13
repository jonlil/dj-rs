use std::net::{IpAddr, SocketAddr};
use std::io::Write;

macro_rules! dj_log {
    ($($arg:tt)*) => {{
        let msg = format!($($arg)*);
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open("/tmp/dj-rs.log") {
            let _ = writeln!(f, "{}", msg);
        }
    }};
}
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;
use axum::{Router, routing::get_service, response::IntoResponse, http::StatusCode, middleware, extract::{Request, State}};
use tower_http::services::ServeFile;
use futures::StreamExt;
use rupnp::ssdp::{SearchTarget, URN};
use rupnp::http::Uri;

/// Find the best LAN IP to use for serving media to local devices.
/// Prefers 192.168.x.x, then 10.x.x.x, then falls back to whatever
/// local_ip_address thinks is primary (may be VPN).
pub fn lan_ip() -> Result<IpAddr, String> {
    let ifaces = local_ip_address::list_afinet_netifas()
        .map_err(|e| e.to_string())?;

    // 192.168.x.x first
    for (_, ip) in &ifaces {
        if let IpAddr::V4(v4) = ip {
            let octs = v4.octets();
            if octs[0] == 192 && octs[1] == 168 {
                return Ok(*ip);
            }
        }
    }
    // 10.x.x.x second
    for (_, ip) in &ifaces {
        if let IpAddr::V4(v4) = ip {
            if v4.octets()[0] == 10 {
                return Ok(*ip);
            }
        }
    }
    // fallback
    local_ip_address::local_ip().map_err(|e| e.to_string())
}

/// Returns true if the SSDP multicast route goes through a non-LAN interface
/// (e.g. a VPN), which would prevent discovery from working.
pub fn ssdp_blocked_by_vpn() -> bool {
    // Connect a UDP socket to the SSDP multicast address and check which
    // local address the OS selects for it.
    let sock = std::net::UdpSocket::bind("0.0.0.0:0").ok();
    if let Some(s) = sock {
        if s.connect("239.255.255.250:1900").is_ok() {
            if let Ok(local) = s.local_addr() {
                let ip = local.ip();
                if let IpAddr::V4(v4) = ip {
                    let octs = v4.octets();
                    // If the selected source is not a typical LAN address, SSDP is probably
                    // going through a VPN or other non-local interface.
                    let is_lan = (octs[0] == 192 && octs[1] == 168)
                        || octs[0] == 10
                        || (octs[0] == 172 && octs[1] >= 16 && octs[1] <= 31
                            && octs[2] == 0); // rough Docker/LAN check
                    return !is_lan;
                }
            }
        }
    }
    false
}

const AV_TRANSPORT_URN: URN = URN::service("schemas-upnp-org", "AVTransport", 1);

#[derive(Debug, Clone)]
pub struct Renderer {
    pub friendly_name: String,
    pub location: Uri,
}

#[derive(Clone)]
struct TrackInfo {
    url: String,
    mime: String,
    dlna_pn: String,
    size: u64,
}

#[derive(Clone)]
pub struct DlnaClient {
    pub runtime: Arc<tokio::runtime::Runtime>,
    server_shutdown: Arc<Mutex<Option<oneshot::Sender<()>>>>,
    server_port: Arc<Mutex<Option<u16>>>,
    current_track: Arc<Mutex<Option<TrackInfo>>>,
}

impl DlnaClient {
    pub fn new() -> Self {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("Failed to build tokio runtime");
        DlnaClient {
            runtime: Arc::new(runtime),
            server_shutdown: Arc::new(Mutex::new(None)),
            server_port: Arc::new(Mutex::new(None)),
            current_track: Arc::new(Mutex::new(None)),
        }
    }

    /// Query what formats the renderer supports.
    pub fn get_sink_protocol_info(&self, renderer_location: Uri) -> Result<String, String> {
        self.runtime.block_on(async {
            let device = rupnp::Device::from_url(renderer_location)
                .await
                .map_err(|e| e.to_string())?;
            let service = device
                .find_service(&AV_TRANSPORT_URN)
                .ok_or("AVTransport service not found")?;
            let info = service
                .action(device.url(), "GetCurrentTransportActions", "<InstanceID>0</InstanceID>")
                .await
                .map_err(|e| e.to_string())?;
            Ok(format!("{:?}", info))
        })
    }

    /// Discover all DLNA MediaRenderers on the LAN (3 second search).
    pub fn discover_renderers(&self) -> Result<Vec<Renderer>, String> {
        self.runtime.block_on(async {
            // Search for all UPnP devices, then filter to those that have
            // an AVTransport service (i.e. actual media renderers).
            let search = rupnp::discover(
                &SearchTarget::All,
                std::time::Duration::from_secs(3),
            )
            .await
            .map_err(|e| e.to_string())?;

            futures::pin_mut!(search);

            let mut renderers: Vec<Renderer> = Vec::new();
            while let Some(device) = search.next().await {
                match device {
                    Ok(d) => {
                        if d.find_service(&AV_TRANSPORT_URN).is_some() {
                            let url = d.url().to_string();
                            // Deduplicate — same device can respond multiple times
                            if !renderers.iter().any(|r| r.location.to_string() == url) {
                                renderers.push(Renderer {
                                    friendly_name: d.friendly_name().to_string(),
                                    location: d.url().clone(),
                                });
                            }
                        }
                    }
                    Err(_) => continue,
                }
            }
            Ok(renderers)
        })
    }

    /// Start an HTTP server serving `file_path`.
    /// Returns the URL the TV should use to fetch the file.
    pub fn start_http_server(&self, file_path: PathBuf) -> Result<String, String> {
        self.stop_http_server();

        let ip = lan_ip()?;
        let addr = SocketAddr::new(ip, 0); // port 0 = OS picks one

        let ext = file_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("mp3")
            .to_lowercase();

        // Audio files are wrapped in an MPEG-PS container via ffmpeg so Samsung's
        // video decoder (which works) handles them instead of the broken audio DMR.
        let is_audio = matches!(ext.as_str(), "mp3" | "m4a" | "flac" | "wav" | "aac" | "ogg");
        let (serve_mime, serve_dlna_pn, route_ext) = if is_audio {
            ("video/mpeg", "MPEG_TS_SD_EU_ISO", "ts")
        } else {
            ("video/mpeg", "MPEG_TS_SD_EU_ISO", "ts")
        };
        let route = format!("/track.{}", route_ext);
        let route_clone = route.clone();
        let content_features = format!(
            "DLNA.ORG_PN={};DLNA.ORG_OP=01;DLNA.ORG_CI=1;DLNA.ORG_FLAGS=ED100000000000000000000000000000",
            serve_dlna_pn
        );
        let file_size = std::fs::metadata(&file_path).map(|m| m.len()).unwrap_or(0);

        let (tx, rx) = oneshot::channel::<()>();

        let device_xml = build_device_xml(ip, 0); // port filled in after bind
        let ip_for_ssdp = ip;

        // Shared state for ContentDirectory handler (url set after port is known)
        let current_track_for_cd = Arc::clone(&self.current_track);

        let file_path_for_route = file_path.clone();
        let cf_for_route = content_features.clone();
        let serve_mime_str = serve_mime;
        let app = Router::new()
            .route(&route_clone, axum::routing::get(move |req: Request| {
                let path = file_path_for_route.clone();
                let cf = cf_for_route.clone();
                let mime = serve_mime_str;
                async move {
                    use tokio_util::io::ReaderStream;
                    dj_log!("[DLNA] ffmpeg transcoding started for {:?}", path);
                    let mut cmd = tokio::process::Command::new("ffmpeg")
                        .args([
                            // Audio input
                            "-i", path.to_str().unwrap_or(""),
                            // Dummy black video source
                            "-f", "lavfi", "-i", "color=c=black:size=352x288:rate=25",
                            // Video stream first (required for MPEG-TS/PS muxers)
                            "-map", "1:v", "-map", "0:a",
                            // MPEG-2 video + MP2 audio → MPEG-TS
                            "-c:v", "mpeg2video", "-b:v", "500k", "-r", "25",
                            "-c:a", "mp2", "-b:a", "192k",
                            "-shortest",
                            "-f", "mpegts",
                            "pipe:1",
                        ])
                        .stdout(std::process::Stdio::piped())
                        .stderr(std::process::Stdio::null())
                        .spawn();

                    match cmd {
                        Ok(mut child) => {
                            let stdout = child.stdout.take().unwrap();
                            let stream = ReaderStream::new(stdout);
                            axum::response::Response::builder()
                                .status(200)
                                .header("content-type", mime)
                                .header("transferMode.dlna.org", "Streaming")
                                .header("contentFeatures.dlna.org", cf)
                                .body(axum::body::Body::from_stream(stream))
                                .unwrap()
                        }
                        Err(e) => {
                            dj_log!("[DLNA] ffmpeg spawn failed: {}", e);
                            axum::response::Response::builder()
                                .status(500)
                                .body(axum::body::Body::empty())
                                .unwrap()
                        }
                    }
                }
            }))
            .route("/device.xml", axum::routing::get(move || {
                let xml = device_xml.clone();
                async move { axum::response::Response::builder()
                    .header("content-type", "text/xml; charset=\"utf-8\"")
                    .body(axum::body::Body::from(xml))
                    .unwrap()
                }
            }))
            .route("/cd.xml", axum::routing::get(|| async {
                axum::response::Response::builder()
                    .header("content-type", "text/xml; charset=\"utf-8\"")
                    .body(axum::body::Body::from(cd_scpd_xml()))
                    .unwrap()
            }))
            .route("/cd", axum::routing::post({
                let ct = current_track_for_cd.clone();
                move |body: axum::body::Bytes| {
                    let ct = ct.clone();
                    async move {
                        let track = ct.lock().unwrap().clone();
                        let body_str = std::str::from_utf8(&body).unwrap_or("").to_string();
                        dj_log!("[DLNA] ContentDirectory SOAP:\n{}", body_str);
                        cd_soap_response(track)
                    }
                }
            }))
            .route("/cd/events", axum::routing::any(|| async {
                StatusCode::OK.into_response()
            }))
            .layer(middleware::from_fn_with_state(content_features, dlna_headers));

        let port = self.runtime.block_on(async {
            let listener = tokio::net::TcpListener::bind(addr)
                .await
                .map_err(|e| e.to_string())?;
            let port = listener.local_addr().unwrap().port();

            tokio::spawn(async move {
                axum::serve(listener, app)
                    .with_graceful_shutdown(async { rx.await.ok(); })
                    .await
                    .ok();
            });

            Ok::<u16, String>(port)
        })?;

        *self.server_shutdown.lock().unwrap() = Some(tx);
        *self.server_port.lock().unwrap() = Some(port);

        let url = format!("http://{}:{}{}", ip, port, route);
        dj_log!("[DLNA] HTTP server: {} → {:?}", url, file_path);

        // Store current track info for ContentDirectory Browse responses
        *self.current_track.lock().unwrap() = Some(TrackInfo {
            url: url.clone(),
            mime: serve_mime.to_string(),
            dlna_pn: serve_dlna_pn.to_string(),
            size: file_size,
        });

        // Announce ourselves as a UPnP MediaServer so Samsung trusts the source
        let location = format!("http://{}:{}/device.xml", ip, port);
        send_ssdp_alive(ip_for_ssdp, &location);
        Ok(url)
    }

    pub fn stop_http_server(&self) {
        if let Some(tx) = self.server_shutdown.lock().unwrap().take() {
            let _ = tx.send(());
        }
        *self.server_port.lock().unwrap() = None;
    }

    /// Set the track URI on a renderer without starting playback.
    pub fn set_uri_on_renderer(&self, renderer_location: Uri, track_url: String) -> Result<(), String> {
        self.runtime.block_on(async {
            let device = rupnp::Device::from_url(renderer_location)
                .await
                .map_err(|e| e.to_string())?;
            let service = device
                .find_service(&AV_TRANSPORT_URN)
                .ok_or("AVTransport service not found on renderer")?;
            // Stop any current playback first — some Samsung models require this
            let _ = service.action(device.url(), "Stop", "<InstanceID>0</InstanceID>").await;
            let metadata = didl_metadata(&track_url);
            let soap_args = format!(
                "<InstanceID>0</InstanceID><CurrentURI>{}</CurrentURI><CurrentURIMetaData>{}</CurrentURIMetaData>",
                xml_escape(&track_url),
                xml_escape(&metadata),
            );
            dj_log!("[DLNA] SOAP args:\n{}", soap_args);
            service
                .action(device.url(), "SetAVTransportURI", &soap_args)
                .await
                .map_err(|e| e.to_string())?;
            // Query what actions are available and what formats the TV supports
            if let Ok(info) = service
                .action(device.url(), "GetCurrentTransportActions", "<InstanceID>0</InstanceID>")
                .await
            {
                dj_log!("[DLNA] GetCurrentTransportActions: {:?}", info);
            }
            // Query ConnectionManager for supported sink protocols
            const CM_URN: rupnp::ssdp::URN = rupnp::ssdp::URN::service("schemas-upnp-org", "ConnectionManager", 1);
            if let Some(cm) = device.find_service(&CM_URN) {
                if let Ok(info) = cm.action(device.url(), "GetProtocolInfo", "").await {
                    dj_log!("[DLNA] ConnectionManager GetProtocolInfo: {:?}", info);
                }
            }
            Ok(())
        })
    }

    /// Set the track URI on a renderer and start playback.
    pub fn play_on_renderer(&self, renderer_location: Uri, track_url: String) -> Result<(), String> {
        self.set_uri_on_renderer(renderer_location.clone(), track_url)?;
        self.runtime.block_on(async {
            let device = rupnp::Device::from_url(renderer_location)
                .await
                .map_err(|e| e.to_string())?;
            let service = device
                .find_service(&AV_TRANSPORT_URN)
                .ok_or("AVTransport service not found on renderer")?;
            service
                .action(device.url(), "Play", "<InstanceID>0</InstanceID><Speed>1</Speed>")
                .await
                .map_err(|e| e.to_string())?;
            Ok(())
        })
    }

    pub fn resume_renderer(&self, renderer_location: Uri) -> Result<(), String> {
        self.runtime.block_on(async {
            let device = rupnp::Device::from_url(renderer_location)
                .await
                .map_err(|e| e.to_string())?;
            let service = device
                .find_service(&AV_TRANSPORT_URN)
                .ok_or("AVTransport service not found")?;
            service
                .action(device.url(), "Play", "<InstanceID>0</InstanceID><Speed>1</Speed>")
                .await
                .map_err(|e| e.to_string())?;
            // Poll transport state after play to capture any error the TV reports
            for _ in 0..3 {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                if let Ok(info) = service
                    .action(device.url(), "GetTransportInfo", "<InstanceID>0</InstanceID>")
                    .await
                {
                    dj_log!("[DLNA] GetTransportInfo: {:?}", info);
                }
                if let Ok(info) = service
                    .action(device.url(), "GetMediaInfo", "<InstanceID>0</InstanceID>")
                    .await
                {
                    dj_log!("[DLNA] GetMediaInfo: {:?}", info);
                }
            }
            Ok(())
        })
    }

    pub fn pause_renderer(&self, renderer_location: Uri) -> Result<(), String> {
        self.runtime.block_on(async {
            let device = rupnp::Device::from_url(renderer_location)
                .await
                .map_err(|e| e.to_string())?;
            let service = device
                .find_service(&AV_TRANSPORT_URN)
                .ok_or("AVTransport service not found")?;
            service
                .action(device.url(), "Pause", "<InstanceID>0</InstanceID>")
                .await
                .map_err(|e| e.to_string())?;
            Ok(())
        })
    }

    pub fn stop_renderer(&self, renderer_location: Uri) -> Result<(), String> {
        self.runtime.block_on(async {
            let device = rupnp::Device::from_url(renderer_location)
                .await
                .map_err(|e| e.to_string())?;
            let service = device
                .find_service(&AV_TRANSPORT_URN)
                .ok_or("AVTransport service not found")?;
            service
                .action(device.url(), "Stop", "<InstanceID>0</InstanceID>")
                .await
                .map_err(|e| e.to_string())?;
            Ok(())
        })
    }
}

fn build_device_xml(ip: IpAddr, _port: u16) -> String {
    format!(
        concat!(
            r#"<?xml version="1.0"?>"#,
            r#"<root xmlns="urn:schemas-upnp-org:device-1-0">"#,
            r#"<specVersion><major>1</major><minor>0</minor></specVersion>"#,
            r#"<device>"#,
            r#"<deviceType>urn:schemas-upnp-org:device:MediaServer:1</deviceType>"#,
            r#"<friendlyName>dj-rs</friendlyName>"#,
            r#"<manufacturer>dj-rs</manufacturer>"#,
            r#"<modelName>dj-rs</modelName>"#,
            r#"<UDN>uuid:dj-rs-{}</UDN>"#,
            r#"<serviceList>"#,
            r#"<service>"#,
            r#"<serviceType>urn:schemas-upnp-org:service:ContentDirectory:1</serviceType>"#,
            r#"<serviceId>urn:upnp-org:serviceId:ContentDirectory</serviceId>"#,
            r#"<SCPDURL>/cd.xml</SCPDURL>"#,
            r#"<controlURL>/cd</controlURL>"#,
            r#"<eventSubURL>/cd/events</eventSubURL>"#,
            r#"</service>"#,
            r#"</serviceList>"#,
            r#"</device></root>"#,
        ),
        ip
    )
}

fn send_ssdp_alive(ip: IpAddr, location: &str) {
    use std::net::UdpSocket;
    let uuid = format!("uuid:dj-rs-{}", ip);
    // Announce: upnp:rootdevice, device UUID, MediaServer:1, ContentDirectory:1
    let packets = [
        format!("NOTIFY * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nCACHE-CONTROL: max-age=1800\r\nLOCATION: {location}\r\nNT: upnp:rootdevice\r\nNTS: ssdp:alive\r\nSERVER: Linux/5.0 UPnP/1.0 dj-rs/0.1\r\nUSN: {uuid}::upnp:rootdevice\r\n\r\n"),
        format!("NOTIFY * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nCACHE-CONTROL: max-age=1800\r\nLOCATION: {location}\r\nNT: {uuid}\r\nNTS: ssdp:alive\r\nSERVER: Linux/5.0 UPnP/1.0 dj-rs/0.1\r\nUSN: {uuid}\r\n\r\n"),
        format!("NOTIFY * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nCACHE-CONTROL: max-age=1800\r\nLOCATION: {location}\r\nNT: urn:schemas-upnp-org:device:MediaServer:1\r\nNTS: ssdp:alive\r\nSERVER: Linux/5.0 UPnP/1.0 dj-rs/0.1\r\nUSN: {uuid}::urn:schemas-upnp-org:device:MediaServer:1\r\n\r\n"),
        format!("NOTIFY * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nCACHE-CONTROL: max-age=1800\r\nLOCATION: {location}\r\nNT: urn:schemas-upnp-org:service:ContentDirectory:1\r\nNTS: ssdp:alive\r\nSERVER: Linux/5.0 UPnP/1.0 dj-rs/0.1\r\nUSN: {uuid}::urn:schemas-upnp-org:service:ContentDirectory:1\r\n\r\n"),
    ];
    if let Ok(sock) = UdpSocket::bind(SocketAddr::new(ip, 0)) {
        for pkt in &packets {
            let _ = sock.send_to(pkt.as_bytes(), "239.255.255.250:1900");
            let _ = sock.send_to(pkt.as_bytes(), "239.255.255.250:1900");
        }
        dj_log!("[DLNA] SSDP alive sent (4 NTs), location={}", location);
    }
}

async fn dlna_headers(
    State(content_features): State<String>,
    req: Request,
    next: middleware::Next,
) -> axum::response::Response {
    let method = req.method().clone();
    let uri    = req.uri().clone();
    let range  = req.headers().get("range").and_then(|v| v.to_str().ok()).unwrap_or("-").to_string();
    let user_agent = req.headers().get("user-agent").and_then(|v| v.to_str().ok()).unwrap_or("-").to_string();
    let mut resp = next.run(req).await;
    let ct = resp.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("-").to_string();
    dj_log!("[DLNA] {} {} range={} ua={} → {} ct={}", method, uri, range, user_agent, resp.status(), ct);
    resp
}

fn didl_metadata(uri: &str) -> String {
    // Audio files are served as MPEG-PS video containers via ffmpeg transcoding
    let (mime, dlna_pn, upnp_class) = if uri.ends_with(".ts") {
        ("video/mpeg", "MPEG_TS_SD_EU_ISO", "object.item.videoItem")
    } else if uri.ends_with(".mp4") {
        ("video/mp4", "AVC_MP4_MP_SD_AAC_MULT5", "object.item.videoItem")
    } else {
        ("video/mpeg", "MPEG_TS_SD_EU_ISO", "object.item.videoItem")
    };

    let protocol_info = format!(
        "http-get:*:{}:DLNA.ORG_PN={};DLNA.ORG_OP=01;DLNA.ORG_CI=1;DLNA.ORG_FLAGS=ED100000000000000000000000000000",
        mime, dlna_pn
    );
    format!(
        concat!(
            r#"<DIDL-Lite xmlns="urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/""#,
            r#" xmlns:dc="http://purl.org/dc/elements/1.1/""#,
            r#" xmlns:upnp="urn:schemas-upnp-org:metadata-1-0/upnp/">"#,
            r#"<item id="1" parentID="0" restricted="1">"#,
            r#"<dc:title>Track</dc:title>"#,
            r#"<upnp:class>{}</upnp:class>"#,
            r#"<res protocolInfo="{}">{}</res>"#,
            r#"</item></DIDL-Lite>"#,
        ),
        upnp_class,
        xml_escape(&protocol_info),
        xml_escape(uri),
    )
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn cd_scpd_xml() -> String {
    concat!(
        r#"<?xml version="1.0"?>"#,
        r#"<scpd xmlns="urn:schemas-upnp-org:service-1-0">"#,
        r#"<specVersion><major>1</major><minor>0</minor></specVersion>"#,
        r#"<actionList>"#,
        r#"<action><name>Browse</name>"#,
        r#"<argumentList>"#,
        r#"<argument><name>ObjectID</name><direction>in</direction><relatedStateVariable>A_ARG_TYPE_ObjectID</relatedStateVariable></argument>"#,
        r#"<argument><name>BrowseFlag</name><direction>in</direction><relatedStateVariable>A_ARG_TYPE_BrowseFlag</relatedStateVariable></argument>"#,
        r#"<argument><name>Filter</name><direction>in</direction><relatedStateVariable>A_ARG_TYPE_Filter</relatedStateVariable></argument>"#,
        r#"<argument><name>StartingIndex</name><direction>in</direction><relatedStateVariable>A_ARG_TYPE_Index</relatedStateVariable></argument>"#,
        r#"<argument><name>RequestedCount</name><direction>in</direction><relatedStateVariable>A_ARG_TYPE_Count</relatedStateVariable></argument>"#,
        r#"<argument><name>SortCriteria</name><direction>in</direction><relatedStateVariable>A_ARG_TYPE_SortCriteria</relatedStateVariable></argument>"#,
        r#"<argument><name>Result</name><direction>out</direction><relatedStateVariable>A_ARG_TYPE_Result</relatedStateVariable></argument>"#,
        r#"<argument><name>NumberReturned</name><direction>out</direction><relatedStateVariable>A_ARG_TYPE_Count</relatedStateVariable></argument>"#,
        r#"<argument><name>TotalMatches</name><direction>out</direction><relatedStateVariable>A_ARG_TYPE_Count</relatedStateVariable></argument>"#,
        r#"<argument><name>UpdateID</name><direction>out</direction><relatedStateVariable>A_ARG_TYPE_UpdateID</relatedStateVariable></argument>"#,
        r#"</argumentList></action>"#,
        r#"</actionList>"#,
        r#"<serviceStateTable>"#,
        r#"<stateVariable sendEvents="no"><name>A_ARG_TYPE_ObjectID</name><dataType>string</dataType></stateVariable>"#,
        r#"<stateVariable sendEvents="no"><name>A_ARG_TYPE_Result</name><dataType>string</dataType></stateVariable>"#,
        r#"<stateVariable sendEvents="no"><name>A_ARG_TYPE_BrowseFlag</name><dataType>string</dataType><allowedValueList><allowedValue>BrowseMetadata</allowedValue><allowedValue>BrowseDirectChildren</allowedValue></allowedValueList></stateVariable>"#,
        r#"<stateVariable sendEvents="no"><name>A_ARG_TYPE_Filter</name><dataType>string</dataType></stateVariable>"#,
        r#"<stateVariable sendEvents="no"><name>A_ARG_TYPE_SortCriteria</name><dataType>string</dataType></stateVariable>"#,
        r#"<stateVariable sendEvents="no"><name>A_ARG_TYPE_Index</name><dataType>ui4</dataType></stateVariable>"#,
        r#"<stateVariable sendEvents="no"><name>A_ARG_TYPE_Count</name><dataType>ui4</dataType></stateVariable>"#,
        r#"<stateVariable sendEvents="no"><name>A_ARG_TYPE_UpdateID</name><dataType>ui4</dataType></stateVariable>"#,
        r#"<stateVariable sendEvents="yes"><name>SystemUpdateID</name><dataType>ui4</dataType></stateVariable>"#,
        r#"<stateVariable sendEvents="yes"><name>ContainerUpdateIDs</name><dataType>string</dataType></stateVariable>"#,
        r#"</serviceStateTable>"#,
        r#"</scpd>"#,
    ).to_string()
}

fn cd_soap_response(track: Option<TrackInfo>) -> axum::response::Response {
    let (didl, count) = match track {
        None => (
            r#"<DIDL-Lite xmlns="urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:upnp="urn:schemas-upnp-org:metadata-1-0/upnp/"></DIDL-Lite>"#.to_string(),
            0u32,
        ),
        Some(t) => {
            let protocol_info = format!(
                "http-get:*:{}:DLNA.ORG_PN={};DLNA.ORG_FLAGS=ED100000000000000000000000000000",
                t.mime, t.dlna_pn
            );
            let item = format!(
                concat!(
                    r#"<DIDL-Lite xmlns="urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/""#,
                    r#" xmlns:dc="http://purl.org/dc/elements/1.1/""#,
                    r#" xmlns:upnp="urn:schemas-upnp-org:metadata-1-0/upnp/">"#,
                    r#"<item id="1" parentID="0" restricted="1">"#,
                    r#"<dc:title>Current Track</dc:title>"#,
                    r#"<upnp:class>object.item.audioItem.musicTrack</upnp:class>"#,
                    r#"<res protocolInfo="{}" size="{}">{}</res>"#,
                    r#"</item></DIDL-Lite>"#,
                ),
                xml_escape(&protocol_info),
                t.size,
                xml_escape(&t.url),
            );
            (item, 1u32)
        }
    };

    let body = format!(
        concat!(
            r#"<?xml version="1.0"?>"#,
            r#"<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/""#,
            r#" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">"#,
            r#"<s:Body>"#,
            r#"<u:BrowseResponse xmlns:u="urn:schemas-upnp-org:service:ContentDirectory:1">"#,
            r#"<Result>{}</Result>"#,
            r#"<NumberReturned>{}</NumberReturned>"#,
            r#"<TotalMatches>{}</TotalMatches>"#,
            r#"<UpdateID>1</UpdateID>"#,
            r#"</u:BrowseResponse>"#,
            r#"</s:Body></s:Envelope>"#,
        ),
        xml_escape(&didl),
        count,
        count,
    );

    axum::response::Response::builder()
        .header("content-type", "text/xml; charset=\"utf-8\"")
        .body(axum::body::Body::from(body))
        .unwrap()
}
