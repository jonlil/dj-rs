mod ui;

use ui::App;

fn main() -> iced::Result {
    iced::application(App::new, App::update, App::view)
        .title("dj-rs")
        .theme(App::theme)
        .window_size((1400.0, 600.0))
        .run()
}
