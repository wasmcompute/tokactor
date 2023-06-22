use tokio::runtime::{Builder, Runtime};

enum ThreadName {
    String(String),
    Fn(Box<dyn Fn() -> String + Send + Sync + 'static>),
}

pub struct WorldBuilder {
    /// must be larger then 0
    worker_threads: Option<usize>,
    /// max amount of blocking threads. Are spun up and down depending on the
    /// workload. POssible for all blocking threads to not exist
    ///
    /// must be larger then 0
    max_blocking_threads: Option<usize>,
    /// name the thread, using a string or function
    thread_name: Option<ThreadName>,
    /// The size of the stack for a worker thread
    thread_stack_size: Option<usize>,
    enable_time: bool,
    enable_io: bool,
}

#[allow(clippy::derivable_impls)]
impl Default for WorldBuilder {
    fn default() -> Self {
        Self {
            worker_threads: None,
            max_blocking_threads: None,
            thread_name: None,
            thread_stack_size: None,
            enable_time: false,
            enable_io: false,
        }
    }
}

impl WorldBuilder {
    pub fn new() -> Self {
        // TODO(Alec): Tokio providers hooks into the runtime which would be good
        //             for future monitoring.
        //             - `on_thread_start`   --> thread is created
        //             - `on_thread_stop`    --> thread is stopped
        //             - `on_thread_park`    --> thread is idle
        //             - `on_thread_unpark`  --> thread is no longer idle
        //             - `thread_keep_alive` --> the lifetime that the thread should stay alive for
        WorldBuilder::default()
    }

    pub fn worker_threads(mut self, size: usize) -> Self {
        if size == 0 {
            self.worker_threads = Some(1);
        } else {
            self.worker_threads = Some(size);
        }
        self
    }

    pub fn max_blocking_threads(mut self, size: usize) -> Self {
        if size == 0 {
            self.max_blocking_threads = Some(1);
        } else {
            self.max_blocking_threads = Some(size);
        }
        self
    }

    pub fn thread_name(mut self, name: String) -> Self {
        self.thread_name = Some(ThreadName::String(name));
        self
    }

    pub fn thread_name_fn<F>(mut self, f: F) -> Self
    where
        F: Fn() -> String + Send + Sync + 'static,
    {
        self.thread_name = Some(ThreadName::Fn(Box::new(f)));
        self
    }

    pub fn thread_stack_size(mut self, size: usize) -> Self {
        self.thread_stack_size = Some(size);
        self
    }

    pub fn enable_time(mut self) -> Self {
        self.enable_time = true;
        self
    }

    pub fn enable_io(mut self) -> Self {
        self.enable_io = true;
        self
    }

    pub fn build(self) -> std::io::Result<World> {
        let builder = Builder::new_multi_thread();
        self.unwrap(builder)
    }

    pub fn build_single_thread(self) -> std::io::Result<World> {
        let builder = Builder::new_current_thread();
        self.unwrap(builder)
    }

    fn unwrap(self, mut builder: Builder) -> std::io::Result<World> {
        if let Some(workers) = self.worker_threads {
            Builder::worker_threads(&mut builder, workers);
        }

        if let Some(max) = self.max_blocking_threads {
            Builder::max_blocking_threads(&mut builder, max);
        }

        if let Some(name) = self.thread_name {
            match name {
                ThreadName::String(name) => Builder::thread_name(&mut builder, name),
                ThreadName::Fn(f) => Builder::thread_name_fn(&mut builder, f),
            };
        }

        if let Some(thread_stack_size) = self.thread_stack_size {
            Builder::thread_stack_size(&mut builder, thread_stack_size);
        }

        if self.enable_io {
            Builder::enable_io(&mut builder);
        }

        if self.enable_time {
            Builder::enable_time(&mut builder);
        }

        let rt = builder.build()?;
        Ok(World::from(rt))
    }
}

pub struct World {
    rt: Runtime,
}

impl From<Runtime> for World {
    fn from(value: Runtime) -> Self {
        Self { rt: value }
    }
}

impl World {
    pub fn new() -> std::io::Result<Self> {
        let rt = Runtime::new()?;
        Ok(Self { rt })
    }

    pub fn block(self) {
        self.rt.block_on(async move {})
    }
}
