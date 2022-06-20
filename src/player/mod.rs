use gtk4 as gtk;
use gtk::prelude::*;
use gtk::Image;
use gdk_pixbuf::{Pixbuf, Colorspace};
use waveform::WaveformConfig;
use gtk::glib::Bytes;
use gstreamer::prelude::*;
use gstreamer as gst;

fn play_track(filename: &str) -> Result<(), anyhow::Error> {
    let launch_configuration = format!("playbin uri=\"file:///{}\"", filename);
    let pipeline = gstreamer::parse_launch(&launch_configuration)
        .expect("Failed creating pipeline");
    eprintln!("{:?}", pipeline);

    pipeline.connect("source-setup", false, |_args| {
        eprintln!("So this happen");
        None
    });

    let bus = pipeline.bus().expect("Failed reading element bus");

    // Instruct the bus to emit signals for each received message, and connect to the interesting signals
    #[allow(clippy::single_match)]
    bus.connect_message(Some("error"), move |_, msg| match msg.view() {
        gstreamer::MessageView::Error(err) => {
            eprintln!(
                "Error received from element {:?}: {}",
                err.src().map(|s| s.path_string()),
                err.error()
            );
            eprintln!("Debugging information: {:?}", err.debug());
        }
        _ => unreachable!(),
    });
    bus.connect_message(Some("buffering"), |_, msg| {
        eprintln!("{:?}", msg);
    });
    bus.add_signal_watch();

    // Start playing
    pipeline.set_state(gstreamer::State::Playing)?;
    let _msg = bus.timed_pop_filtered(
        gst::ClockTime::NONE,
        &[gst::MessageType::Error, gst::MessageType::Eos],
    );

    pipeline.set_state(gstreamer::State::Null)?;
    bus.remove_signal_watch();
    Ok(())
}

pub fn build(container: &gtk::Box) {
    let dialog = gtk::FileChooserNative::new(
        Some("Open File"),
        gtk::Window::NONE,
        gtk::FileChooserAction::Open,
        Some("Open"),
        Some("Cancel"),
    );
    dialog.set_modal(true);

    let audio_filter = gtk::FileFilter::new();
    audio_filter.add_mime_type("audio/*");
    audio_filter.set_name(Some("Audio"));
    dialog.add_filter(&audio_filter);

    let button = gtk::Button::builder()
        .label("File picker")
        .build();

    dialog.connect_response(|dlg, response| {
        match response {
            gtk::ResponseType::Accept => {
                match dlg.file() {
                    Some(file) => {
                        let path = file.path().expect("Failure");
                        std::thread::spawn(move || {
                            play_track(&path.to_str().unwrap()).expect("Failed");
                        });
                        dlg.destroy();
                    }
                    _ => panic!("Fucke"),
                };
            },
            _ => {},
        };
    });

    button.connect_clicked(move |_| {
        dialog.show();
    });

    container.append(&button);
}

fn waveform_generator() -> Image {
    let mut samples: Vec<f64> = Vec::new();
    for t in 0..44100 {
        samples.push(
            ((t as f64) / 100f64 * 2f64 * 3.1415f64).sin() * ((t as f64) / 10000f64 * 2f64 * 3.1415f64).sin(),
        );
    }
    let waveform_config = WaveformConfig::new(
        -1f64,
        1f64,
        // Foreground color
        waveform::Color::Vector4(0, 0, 0, 255),
        // Background color
        waveform::Color::Vector4(0, 0, 0, 0),
    ).expect("Failed creating waveform config");
    let sample_sequence = waveform::SampleSequence {
        data: &samples[..],
        sample_rate: 44100f64,
    };
    let waveform_rendered = waveform::BinnedWaveformRenderer::new(
        &sample_sequence,
        10,
        waveform_config,
    ).expect("Failed creating waveform renderer");
    let vec: Vec<u8> = waveform_rendered.render_vec(
        waveform::TimeRange::Seconds(0.0f64, 1.0f64), (800, 100)
    ).expect("Failed render");
    let pixbuf = Pixbuf::from_bytes(
        &Bytes::from(&vec), Colorspace::Rgb, true, 8, 800, 100, 800 * 4);

    Image::from_pixbuf(Some(&pixbuf))

}
