// Scratch test tool: moves a virtual pointer, clicks, or scrolls, on a wlroots compositor (zwlr_virtual_pointer_v1).
// usage: vptr move X Y | click X Y | wheel X Y DELTA   (output assumed 1024x768)
use wayland_client::{protocol::{wl_registry, wl_seat}, Connection, Dispatch, QueueHandle, globals::{registry_queue_init, GlobalListContents}};
use wayland_protocols_wlr::virtual_pointer::v1::client::{zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1, zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1};
struct S;
impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for S { fn event(_: &mut Self, _: &wl_registry::WlRegistry, _: wl_registry::Event, _: &GlobalListContents, _: &Connection, _: &QueueHandle<Self>) {} }
impl Dispatch<wl_seat::WlSeat, ()> for S { fn event(_: &mut Self, _: &wl_seat::WlSeat, _: wl_seat::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {} }
impl Dispatch<ZwlrVirtualPointerManagerV1, ()> for S { fn event(_: &mut Self, _: &ZwlrVirtualPointerManagerV1, _: <ZwlrVirtualPointerManagerV1 as wayland_client::Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {} }
impl Dispatch<ZwlrVirtualPointerV1, ()> for S { fn event(_: &mut Self, _: &ZwlrVirtualPointerV1, _: <ZwlrVirtualPointerV1 as wayland_client::Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {} }
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let conn = Connection::connect_to_env().unwrap();
    let (g, mut q) = registry_queue_init::<S>(&conn).unwrap();
    let qh = q.handle();
    let seat: wl_seat::WlSeat = g.bind(&qh, 1..=1, ()).unwrap();
    let mgr: ZwlrVirtualPointerManagerV1 = g.bind(&qh, 1..=2, ()).unwrap();
    let p = mgr.create_virtual_pointer(Some(&seat), &qh, ());
    let n = |i: usize| a[i].parse::<u32>().unwrap();
    let mut t = 0u32;
    let mut tick = || { t += 20; t };
    let mv = |p: &ZwlrVirtualPointerV1, x: u32, y: u32, t: u32| { p.motion_absolute(t, x, y, 1024, 768); p.frame(); };
    let mut s = S;
    match a[1].as_str() {
        "move" => mv(&p, n(2), n(3), tick()),
        "click" => { mv(&p, n(2), n(3), tick()); q.roundtrip(&mut s).unwrap(); std::thread::sleep(std::time::Duration::from_millis(300)); p.button(tick(), 0x110, wayland_client::protocol::wl_pointer::ButtonState::Pressed); p.frame(); q.roundtrip(&mut s).unwrap(); p.button(tick(), 0x110, wayland_client::protocol::wl_pointer::ButtonState::Released); p.frame(); }
        "wheel" => { mv(&p, n(2), n(3), tick()); q.roundtrip(&mut s).unwrap(); std::thread::sleep(std::time::Duration::from_millis(300)); let d = a[4].parse::<i32>().unwrap(); p.axis_source(wayland_client::protocol::wl_pointer::AxisSource::Wheel); p.axis_discrete(tick(), wayland_client::protocol::wl_pointer::Axis::VerticalScroll, d as f64 * 15.0, d); p.frame(); }
        _ => {}
    }
    q.roundtrip(&mut s).unwrap();
}
