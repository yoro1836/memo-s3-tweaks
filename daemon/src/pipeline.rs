/// Unified event passed through the plugin pipeline.
#[derive(Debug, Clone)]
pub enum Event {
    /// Analog stick input: x, y, side
    Stick { x: i32, y: i32, side: Side },
    /// Trigger input (ABS_Z=LT, ABS_RZ=RT)
    Trigger { value: i32, side: Side },
    /// Button input (EV_KEY event)
    Button { code: u16, pressed: bool },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Side { Left, Right }

impl Side {
    pub fn as_str(&self) -> &'static str {
        match self { Side::Left => "left", Side::Right => "right" }
    }
}

/// Emitted output event from plugins.
#[derive(Debug, Clone)]
pub struct EmitEvent {
    pub ev_type: u16,   // EV_ABS or EV_KEY
    pub code: u16,
    pub value: i32,
    /// If set and value==1 (KEY down), schedule a release after this many ms.
    pub hold_ms: Option<u64>,
}

/// Pipeline context including settings and emit buffer.
pub struct Ctx {
    pub settings: std::collections::HashMap<String, String>,
    pub emits: Vec<EmitEvent>,
    pub drop_original: bool,
}

/// Trait for a processing step.
pub trait Processor: Send + Sync {
    #[allow(dead_code)]
    fn id(&self) -> &str;
    fn process(&self, event: &mut Event, ctx: &mut Ctx);
}

/// Ordered pipeline.
pub struct Pipeline {
    steps: Vec<Box<dyn Processor>>,
}

impl Pipeline {
    pub fn new() -> Self { Pipeline { steps: Vec::new() } }

    pub fn add(&mut self, p: Box<dyn Processor>) { self.steps.push(p); }

    pub fn run(&self, event: &mut Event, settings: &std::collections::HashMap<String, String>) -> (Vec<EmitEvent>, bool) {
        let mut ctx = Ctx { settings: settings.clone(), emits: Vec::new(), drop_original: false };
        for step in &self.steps {
            step.process(event, &mut ctx);
        }
        (ctx.emits, ctx.drop_original)
    }

    #[allow(dead_code)]
    pub fn plugin_ids(&self) -> Vec<String> {
        self.steps.iter().map(|s| s.id().to_string()).collect()
    }
}
