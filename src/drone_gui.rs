use crossbeam_channel::Sender;
use crossbeam_channel::{unbounded, Receiver};
use lazy_static::lazy_static;
use std::collections::VecDeque;
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, SystemTime};
use std::{collections::HashMap, io::Cursor, mem, sync::Mutex, thread, thread::JoinHandle};
use tiny_http::{Header, Method, Request, Response, Server};
use tungstenite::{Message, WebSocket};
use wg_2024::network::NodeId;

lazy_static! {
    static ref SENDER: Mutex<Option<Sender<GUIMessage>>> = Mutex::new(None);
}
lazy_static! {
    static ref GUIS: Mutex<HashMap<NodeId, DroneGUI>> = Mutex::new(HashMap::new());
}
lazy_static! {
    static ref SERVER_JOIN_HANDLES: Mutex<Vec<JoinHandle<()>>> = Mutex::new(Vec::new());
}

const DRONE_GUI_PORT: u16 = 8463;

pub enum GUIMessage {
    DroneAdded(NodeId, f32),
    DroneRemoved(NodeId),
    PDRChanged(NodeId, f32),
    BagelDropped(NodeId, bool),
}

pub fn add_gui(id: NodeId, pdr: f32, sender: &mut Option<Sender<GUIMessage>>) {
    let mut static_sender = SENDER.lock().unwrap();
    if let Some(static_sender) = static_sender.as_ref() {
        static_sender.send(GUIMessage::DroneAdded(id, pdr)).ok();
        *sender = Some(static_sender.clone());
    } else {
        let (send, recv) = unbounded();
        send.send(GUIMessage::DroneAdded(id, pdr)).ok();
        *sender = Some(send.clone());
        *static_sender = Some(send);
        run(recv);
    }
}

pub fn remove_gui(id: NodeId, sender: &mut Option<Sender<GUIMessage>>) {
    if GUIS.lock().unwrap().len() == 1 {
        *GUIS.lock().unwrap() = HashMap::new();
        *SENDER.lock().unwrap() = None;
        let mut server_join_handles = SERVER_JOIN_HANDLES.lock().unwrap();
        for handle in mem::take(&mut *server_join_handles).into_iter() {
            handle.join().ok();
        }
    } else if let Some(sender) = sender.as_ref() {
        sender.send(GUIMessage::DroneRemoved(id)).ok();
    }
}

pub fn change_pdr(id: NodeId, pdr: f32, sender: &Option<Sender<GUIMessage>>) {
    if let Some(sender) = sender.as_ref() {
        sender.send(GUIMessage::PDRChanged(id, pdr)).ok();
    }
}

pub fn drop_bagel(id: NodeId, dropped: bool, sender: &Option<Sender<GUIMessage>>) {
    if let Some(sender) = sender.as_ref() {
        sender.send(GUIMessage::BagelDropped(id, dropped)).ok();
    }
}

fn run(receiver: Receiver<GUIMessage>) {
    let receiver_handle = thread::spawn(move || {
        receiver_daemon(receiver);
    });
    let http_daemon_handle = thread::spawn(move || {
        http_daemon();
    });
    let web_socket_daemon_handle = thread::spawn(move || {
        web_socket_daemon();
    });
    let mut server_join_handles = SERVER_JOIN_HANDLES.lock().unwrap();
    server_join_handles.push(receiver_handle);
    server_join_handles.push(http_daemon_handle);
    server_join_handles.push(web_socket_daemon_handle);
}

fn receiver_daemon(receiver: Receiver<GUIMessage>) {
    for message in receiver {
        handle_message(message);
        if GUIS.lock().unwrap().is_empty() {
            break;
        }
    }
    *SENDER.lock().unwrap() = None;
}

fn http_daemon() {
    println!("Visit http://localhost:{}", DRONE_GUI_PORT);
    let http_server = Server::http(format!("0.0.0.0:{}", DRONE_GUI_PORT)).unwrap();
    loop {
        if let Ok(Some(request)) = http_server.try_recv() {
            handle_http_request(request, GUIS.lock().unwrap().clone());
        }

        if SENDER.lock().unwrap().is_none() {
            break;
        }
    }

    #[cfg(feature = "debug")]
    println!("HTTP server shutting down");
}

fn web_socket_daemon() {
    let tcp_listener = TcpListener::bind(format!("0.0.0.0:{}", DRONE_GUI_PORT + 1)).unwrap();
    tcp_listener.set_nonblocking(true).ok();
    let starting_time = SystemTime::now() - Duration::from_secs(5);
    loop {
        let stream = tcp_listener.accept();
        match stream {
            Ok((stream, _)) => {
                let web_socket_updates = thread::spawn(move || {
                    if let Ok(web_socket_message) = tungstenite::accept(stream) {
                        handle_web_socket_connection(web_socket_message, starting_time);
                    }
                });
                SERVER_JOIN_HANDLES.lock().unwrap().push(web_socket_updates);
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(_) => {}
        }

        if SENDER.lock().unwrap().is_none() {
            break;
        }
    }

    #[cfg(feature = "debug")]
    println!("WebSocket server shutting down");
}

fn handle_web_socket_connection(mut web_socket: WebSocket<TcpStream>, starting_time: SystemTime) {
    let message = loop {
        if let Ok(message) = web_socket.read() {
            break message;
        }
        thread::sleep(Duration::from_millis(10));
    };

    if let Ok(text) = message.into_text() {
        if let Ok(id) = text.parse::<NodeId>() {
            let mut last_check = SystemTime::now() - Duration::from_secs(10);
            let mut last_pdr = 0.0;
            while let Some(gui) = GUIS.lock().unwrap().get(&id) {
                let drops = gui
                    .drops
                    .iter()
                    .filter(|drop| drop.time > last_check)
                    .map(|drop| {
                        format!(
                            "{{ \"exploded\": {}, \"time\": {} }}",
                            drop.exploded,
                            drop.time
                                .duration_since(starting_time)
                                .unwrap()
                                .as_secs_f32()
                        )
                    })
                    .collect::<Vec<String>>()
                    .join(", ");

                if gui.pdr != last_pdr || !drops.is_empty() {
                    let response = format!("{{ \"pdr\": {}, \"drops\": [ {} ] }}", gui.pdr, drops);

                    if web_socket.write(Message::Text(response.into())).is_err() {
                        break;
                    }

                    web_socket.flush().ok();

                    last_check = SystemTime::now();
                    last_pdr = gui.pdr;
                }

                thread::sleep(Duration::from_millis(200));
            }

            // TODO: this doesn't seem to work
            #[cfg(feature = "debug")]
            println!("WebSocket connection closed");
        }
    }
}

fn handle_http_request(request: Request, guis: HashMap<NodeId, DroneGUI>) {
    let handle = thread::spawn(move || {
        let method = request.method();
        let path = request.url();

        let response = match (method, path) {
            (Method::Get, "/") => handle_root(guis),
            (Method::Get, "/style") => handle_style(),
            (Method::Get, "/script") => handle_script(),
            (Method::Get, "/bagel.png") => handle_icon(),
            (Method::Get, path)
                if path.starts_with("/")
                    && path[1..].parse::<NodeId>().is_ok()
                    && guis.contains_key(&path[1..].parse().unwrap()) =>
            {
                let id = path[1..].parse::<NodeId>().unwrap();
                let drone_gui = guis.get(&id).unwrap();
                handle_drone(drone_gui)
            }
            _ => handle_not_found(),
        };

        request.respond(response).ok();
    });

    SERVER_JOIN_HANDLES.lock().unwrap().push(handle);
}

fn handle_root(guis: HashMap<NodeId, DroneGUI>) -> Response<Cursor<Vec<u8>>> {
    let html_body = format!(
        "<h1>Bagel Bomber GUI</h1><div class=\"drone-list\">{}</div>",
        guis.values().map(|gui| gui.anchor()).collect::<String>()
    );
    Response::from_string(wrap_html("Bagel Bomber GUI", html_body))
        .with_header("Content-Type: text/html".parse::<Header>().unwrap())
}

fn icon_file() -> &'static [u8] {
    include_bytes!("../assets/bagel.png")
}

fn handle_icon() -> Response<Cursor<Vec<u8>>> {
    Response::from_data(icon_file())
        .with_header("Content-Type: image/png".parse::<Header>().unwrap())
}

fn style_file() -> &'static str {
    include_str!("../assets/style.css")
}

fn handle_style() -> Response<Cursor<Vec<u8>>> {
    Response::from_string(style_file()).with_header("Content-Type: text/css".parse::<Header>().unwrap())
}

fn handle_drone(drone_gui: &DroneGUI) -> Response<Cursor<Vec<u8>>> {
    drone_gui.drone_page()
}

fn script_file() -> &'static str {
    include_str!("../assets/script.js")
}

fn handle_script() -> Response<Cursor<Vec<u8>>> {
    Response::from_string(script_file())
        .with_header("Content-Type: text/javascript".parse::<Header>().unwrap())
}

fn handle_not_found() -> Response<Cursor<Vec<u8>>> {
    let html_body = "<h1>Not Found</h1>".to_string();
    Response::from_string(wrap_html("Not Found", html_body))
        .with_status_code(404)
        .with_header("Content-Type: text/html".parse::<Header>().unwrap())
}

fn wrap_html(title: &str, body: String) -> String {
    format!(
        r#"
<!DOCTYPE html>
<html>
<head>
    <meta charset="UTF-8">
    <title>{title}</title>
    <link rel="icon" href="/bagel.png"/>
    <link rel="stylesheet" href="/style"/>
</head>
<body>
    {body}
</body>
</html>
"#
    )
}

fn handle_message(message: GUIMessage) {
    let mut guis = GUIS.lock().unwrap();
    match message {
        GUIMessage::DroneAdded(id, pdr) => {
            guis.insert(id, DroneGUI::new(id, pdr));
        }
        GUIMessage::DroneRemoved(id) => {
            guis.remove(&id);
        }
        GUIMessage::PDRChanged(id, pdr) => {
            if let Some(gui) = guis.get_mut(&id) {
                gui.set_pdr(pdr);
            }
        }
        GUIMessage::BagelDropped(id, dropped) => {
            if let Some(gui) = guis.get_mut(&id) {
                gui.bagel_dropped(dropped);
            }
        }
    }
}

#[derive(Clone, Debug)]
struct Drop {
    exploded: bool,
    time: SystemTime,
}

#[derive(Clone)]
pub struct DroneGUI {
    id: NodeId,
    pdr: f32,
    drops: VecDeque<Drop>,
}

impl DroneGUI {
    pub fn new(id: NodeId, pdr: f32) -> Self {
        DroneGUI {
            id,
            pdr,
            drops: VecDeque::with_capacity(10),
        }
    }

    fn set_pdr(&mut self, pdr: f32) {
        self.pdr = pdr;
    }

    fn bagel_dropped(&mut self, result: bool) {
        if self.drops.len() >= 10 {
            self.drops.pop_front();
        }
        self.drops.push_back(Drop {
            exploded: result,
            time: SystemTime::now(),
        });
    }

    fn anchor(&self) -> String {
        format!(
            "<a class=\"drone-link\" href=\"{}\">Bagel Bomber {}</a>",
            self.url(),
            self.id
        )
    }

    fn url(&self) -> String {
        format!("/{}", self.id)
    }

    fn drone_page(&self) -> Response<Cursor<Vec<u8>>> {
        let html_body = format!(
            r#"
<h1>Bagel Bomber {}</h1>
<div class="container">
    <div id="field" data-id="{}" data-pdr="{}"></div>
    <a class="back-button" href="/">Back to Hub</a>
</div>
<script src="/script" defer></script>
"#,
            self.id, self.id, self.pdr
        );
        Response::from_string(wrap_html(&format!("Bagel Bomber {}", self.id), html_body))
            .with_header("Content-Type: text/html".parse::<Header>().unwrap())
    }
}
