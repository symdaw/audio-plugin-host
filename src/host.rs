#[derive(Default, Clone, Debug)]
pub struct Host {
    pub name: &'static str,
    pub version: &'static str,
    pub vendor: &'static str,
    pub url: &'static str,
    pub knob_preference: Option<KnobPreference>,
    pub language: Option<Language>,
    #[cfg(feature = "future_thread_pool")]
    pub thread_pool_hander: Option<fn(callback: Box<dyn std::future::Future<Output = ()>>)>,
    #[cfg(not(feature = "future_thread_pool"))]
    pub thread_pool_hander: Option<fn(callback: Box<dyn Fn(usize)>, count: usize)>,
}

impl Host {
    pub fn new(name: &'static str, version: &'static str, vendor: &'static str) -> Self {
        Self {
            name,
            version,
            vendor,
            ..Default::default()
        }
    }
}

#[derive(Clone, Debug, Copy)]
pub enum KnobPreference {
    Circular,
    Linear,
}

// TODO
#[derive(Clone, Debug, Default, Copy)]
pub enum Language {
    #[default]
    English,
    Spanish,
    French,
    German,
    Italian,
}
