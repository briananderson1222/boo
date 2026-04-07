use std::sync::mpsc;

/// A request to show a notification.
pub struct NotifyRequest {
    pub summary: String,
    pub body: String,
    pub open: Option<String>,
    pub working_dir: Option<String>,
    pub job_name: Option<String>,
}

/// Handle to send notification requests to the notification thread.
#[derive(Clone)]
pub struct NotificationSender {
    tx: mpsc::Sender<NotifyRequest>,
    rx_holder: std::sync::Arc<std::sync::Mutex<Option<mpsc::Receiver<NotifyRequest>>>>,
}

impl NotificationSender {
    /// Create the sender. Call `run_loop()` on the main thread afterward.
    pub fn start_on_main_thread() -> Self {
        let (tx, rx) = mpsc::channel::<NotifyRequest>();
        Self {
            tx,
            rx_holder: std::sync::Arc::new(std::sync::Mutex::new(Some(rx))),
        }
    }

    /// For non-daemon use (spawns a background thread).
    pub fn start() -> Self {
        let s = Self::start_on_main_thread();
        let s2 = s.clone();
        std::thread::spawn(move || s2.run_loop());
        s
    }

    pub fn send(&self, req: NotifyRequest) {
        let _ = self.tx.send(req);
    }

    /// Run the notification loop. On macOS, must be called from the main thread.
    pub fn run_loop(&self) {
        let rx = self
            .rx_holder
            .lock()
            .unwrap()
            .take()
            .expect("run_loop called twice");
        run_notification_loop(rx);
    }
}

fn run_notification_loop(rx: mpsc::Receiver<NotifyRequest>) {
    use user_notify::{
        NotificationBuilder, NotificationCategory, NotificationCategoryAction,
        NotificationResponseAction,
    };

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    let manager = rt.block_on(async {
        let m = user_notify::get_notification_manager("com.boo.scheduler".into(), None);
        let _ = m.first_time_ask_for_notification_permission().await;
        m
    });

    let _ = manager.register(
        Box::new(move |response| {
            let open_path = response.user_info.get("open").cloned();
            let work_dir = response.user_info.get("working_dir").cloned();

            match &response.action {
                NotificationResponseAction::Default => {
                    if let Some(path) = &open_path {
                        crate::notifier::open_file(path);
                    }
                }
                NotificationResponseAction::Other(id) if id == "reply" => {
                    if let (Some(text), Some(_)) = (&response.user_text, &work_dir) {
                        let text = text.trim();
                        if !text.is_empty() {
                            let job_name = response
                                .user_info
                                .get("job_name")
                                .cloned()
                                .unwrap_or_default();
                            crate::notifier::open_terminal_resume(&job_name, Some(text), false);
                        }
                    }
                }
                _ => {}
            }
        }),
        vec![NotificationCategory {
            identifier: "boo-job".into(),
            actions: vec![NotificationCategoryAction::TextInputAction {
                identifier: "reply".into(),
                title: "Reply".into(),
                input_button_title: "Send".into(),
                input_placeholder: "Follow up...".into(),
            }],
        }],
    );

    #[cfg(target_os = "macos")]
    {
        extern "C" {
            fn CFRunLoopRunInMode(
                mode: *const std::ffi::c_void,
                seconds: f64,
                return_after: u8,
            ) -> i32;
            static kCFRunLoopDefaultMode: *const std::ffi::c_void;
        }
        loop {
            unsafe {
                CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.1, 0);
            }
            while let Ok(req) = rx.try_recv() {
                let mut user_info = std::collections::HashMap::new();
                if let Some(ref path) = req.open {
                    user_info.insert("open".into(), path.clone());
                }
                if let Some(ref dir) = req.working_dir {
                    user_info.insert("working_dir".into(), dir.clone());
                }
                if let Some(ref name) = req.job_name {
                    user_info.insert("job_name".into(), name.clone());
                }
                let n = NotificationBuilder::new()
                    .title(&req.summary)
                    .body(&req.body)
                    .set_category_id("boo-job")
                    .set_user_info(user_info);
                let _ = rt.block_on(manager.send_notification(n));
            }
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        while let Ok(req) = rx.recv() {
            let mut user_info = std::collections::HashMap::new();
            if let Some(ref path) = req.open {
                user_info.insert("open".into(), path.clone());
            }
            if let Some(ref dir) = req.working_dir {
                user_info.insert("working_dir".into(), dir.clone());
            }
            if let Some(ref name) = req.job_name {
                user_info.insert("job_name".into(), name.clone());
            }
            let n = NotificationBuilder::new()
                .title(&req.summary)
                .body(&req.body)
                .set_category_id("boo-job")
                .set_user_info(user_info);
            let _ = rt.block_on(manager.send_notification(n));
        }
    }
}
