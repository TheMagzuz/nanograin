extern crate ladspa;

use std::{cell::RefMut, collections::HashMap};

use iota::iota;
use ladspa::{Data, Plugin, PortConnection, PluginDescriptor, Port, PortDescriptor, DefaultValue, HINT_INTEGER};
use rand::prelude::*;

const MAX_GRAIN_COUNT: Data = 50.0;
const MAX_GRAIN_SIZE: Data = 0.5;

iota!{
const PORT_AUDIO_IN_LEFT: usize = iota;
    , PORT_AUDIO_IN_RIGHT
    , PORT_AUDIO_OUT_LEFT
    , PORT_AUDIO_OUT_RIGHT
    , PORT_DRY_WET
    , PORT_GRAIN_SIZE
    , PORT_GRAIN_COUNT
    , PORT_FEEDBACK
    , PORT_FADE_IN
    , PORT_FADE_OUT
}

struct NanoGrain {
    sample_rate: Data,
    buf: Vec<Vec<(Data, Data, Data)>>,
    grain_read_pos: usize,
    grain_read_idx: usize,
    grain_write_pos: usize,
    grain_write_idx: usize,
}

impl Plugin for NanoGrain {
    fn activate(&mut self) {
        self.buf.clear();
        self.buf.resize_with(MAX_GRAIN_COUNT as usize + 1, || {
            let len = (self.sample_rate * MAX_GRAIN_SIZE) as usize + 1;
            let mut v = Vec::with_capacity(len);
            v.resize(len, (0.0, 0.0, 0.0));
            v
        });

        let mut rng = thread_rng();
        self.grain_read_pos = 0;
        self.grain_read_idx = rng.gen_range(0..MAX_GRAIN_COUNT as usize);

        self.grain_write_pos = 0;
        self.grain_write_idx = rng.gen_range(0..MAX_GRAIN_COUNT as usize);
    }

    fn run<'a>(&mut self, sample_count: usize, ports: &[&'a PortConnection<'a>]) {
        let ports = NanoGrainPorts::new(ports);
        let input = ports.input;
        let mut output = ports.output;
        let grain_size = (ports.grain_size*self.sample_rate) as usize;
        let dry_wet = ports.dry_wet;
        let feedback = ports.feedback;
        let grain_count = ports.grain_count;

        let mut rng = thread_rng();

        for i in 0..sample_count {
            let input_sample = (input.0[i], input.1[i]);
            if self.grain_read_pos >= 24000 {
                println!("grain size: {grain_size}");
            }
            let sample = self.buf[self.grain_read_idx][self.grain_read_pos];
            self.buf[self.grain_read_idx][self.grain_read_pos].2 *= feedback;

            let fb = sample.2;
            let fade_in = (self.grain_read_pos as f32/(grain_size as f32 * ports.fade_in)).min(1.0);
            let fade_out = ((grain_size as f32 - self.grain_read_pos as f32)/(grain_size as f32 * ports.fade_out)).min(1.0);
            let amp = dry_wet * fb * fade_in * fade_out;

            output.0[i] = amp * sample.0 + input_sample.0 * (1.0 - dry_wet);
            output.1[i] = amp * sample.1 + input_sample.1 * (1.0 - dry_wet);

            self.buf[self.grain_write_idx][self.grain_write_pos] = (input_sample.0, input_sample.1, feedback);

            self.grain_write_pos += 1;
            if self.grain_write_pos > grain_size-1 {
                self.grain_write_pos = 0;
                self.grain_write_idx = rng.gen_range(0..grain_count as usize);
            }

            self.grain_read_pos += 1;
            if self.grain_read_pos > grain_size-1 {
                self.grain_read_pos = 0;
                self.grain_read_idx = rng.gen_range(0..grain_count as usize);
            }
        }
    }
}

fn new_nanograin(_: &PluginDescriptor, sample_rate: u64) -> Box<dyn Plugin + Send> {
    Box::new(NanoGrain {
        sample_rate: sample_rate as Data,
        buf: Vec::new(),
        grain_read_pos: 0,
        grain_read_idx: 0,
        grain_write_pos: 0,
        grain_write_idx: 0,
    })
}

macro_rules! port_control {
    ($ports:expr, $idx:expr) => {
        *$ports[$idx].unwrap_control()
    };
}

struct NanoGrainPorts<'a> {
    input: (&'a[Data], &'a[Data]),
    output: (RefMut<'a, &'a mut [f32]>, RefMut<'a, &'a mut [f32]>),
    dry_wet: f32,
    grain_size: f32,
    grain_count: usize,
    feedback: f32,
    fade_in: f32,
    fade_out: f32,
}

impl<'a> NanoGrainPorts<'a> {
    fn new(ports: &[&'a PortConnection<'a>]) -> Self {
        let input = (ports[PORT_AUDIO_IN_LEFT].unwrap_audio(), ports[PORT_AUDIO_IN_RIGHT].unwrap_audio());
        let output = (ports[PORT_AUDIO_OUT_LEFT].unwrap_audio_mut(), ports[PORT_AUDIO_OUT_RIGHT].unwrap_audio_mut());
        let dry_wet = port_control!(ports, PORT_DRY_WET);
        let grain_size = port_control!(ports, PORT_GRAIN_SIZE);
        let grain_count = port_control!(ports, PORT_GRAIN_COUNT) as usize;
        let feedback = port_control!(ports, PORT_FEEDBACK);
        let fade_in = port_control!(ports, PORT_FADE_IN);
        let fade_out = port_control!(ports, PORT_FADE_OUT);

        Self {
            input,
            output,
            dry_wet,
            grain_size,
            grain_count,
            feedback,
            fade_in,
            fade_out,
        }
    }
}

#[no_mangle]
pub extern fn get_ladspa_descriptor(index: u64) -> Option<PluginDescriptor> {
    match index {
        0 => {
            let ports_map = HashMap::from([
                (PORT_AUDIO_IN_LEFT, Port {
                    name: "Left Audio In",
                    desc: PortDescriptor::AudioInput,
                    ..Default::default()
                }),
                (PORT_AUDIO_IN_RIGHT, Port {
                    name: "Right Audio In",
                    desc: PortDescriptor::AudioInput,
                    ..Default::default()
                }),
                (PORT_AUDIO_OUT_LEFT, Port {
                    name: "Left Audio Out",
                    desc: PortDescriptor::AudioOutput,
                    ..Default::default()
                }),
                (PORT_AUDIO_OUT_RIGHT, Port {
                    name: "Right Audio Out",
                    desc: PortDescriptor::AudioOutput,
                    ..Default::default()
                }),
                (PORT_DRY_WET, Port {
                    name: "Dry/Wet",
                    desc: PortDescriptor::ControlInput,
                    hint: None,
                    default: Some(DefaultValue::Middle),
                    lower_bound: Some(0.0),
                    upper_bound: Some(1.0),
                    ..Default::default()
                }),
                (PORT_GRAIN_SIZE, Port {
                    name: "Grain Size (seconds)",
                    desc: PortDescriptor::ControlInput,
                    hint: None,
                    default: Some(DefaultValue::Middle),
                    lower_bound: Some(0.01),
                    upper_bound: Some(MAX_GRAIN_SIZE),
                }),
                (PORT_GRAIN_COUNT, Port {
                    name: "Grain Count",
                    desc: PortDescriptor::ControlInput,
                    hint: Some(HINT_INTEGER),
                    default: Some(DefaultValue::Middle),
                    lower_bound: Some(1.0),
                    upper_bound: Some(MAX_GRAIN_COUNT),
                }),
                (PORT_FEEDBACK, Port {
                    name: "Feedback",
                    desc: PortDescriptor::ControlInput,
                    hint: None,
                    default: Some(DefaultValue::Middle),
                    lower_bound: Some(0.0),
                    upper_bound: Some(1.0),
                    ..Default::default()
                }),
                (PORT_FADE_IN, Port {
                    name: "Fade In",
                    desc: PortDescriptor::ControlInput,
                    hint: None,
                    default: Some(DefaultValue::Low),
                    lower_bound: Some(0.0),
                    upper_bound: Some(1.0),
                    ..Default::default()
                }),
                (PORT_FADE_OUT, Port {
                    name: "Fade Out",
                    desc: PortDescriptor::ControlInput,
                    hint: None,
                    default: Some(DefaultValue::Low),
                    lower_bound: Some(0.0),
                    upper_bound: Some(1.0),
                    ..Default::default()
                }),
            ]);
            let mut ports: Vec<Option<Port>> = Vec::with_capacity(ports_map.len());
            ports.resize_with(ports_map.len(), || None);
            let ports = ports.iter().enumerate().map(|(i, _v)| ports_map[&i]).collect();

            Some(PluginDescriptor {
                unique_id: 4453247696088698671,
                label: "nanograin",
                properties: ladspa::PROP_NONE,
                name: "nanograin",
                maker: "Markus Dam",
                copyright: "None",
                ports,
                new: new_nanograin,
            })
        },
        _ => None
    }
}
