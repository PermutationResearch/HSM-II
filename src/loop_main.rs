use std::time::Duration;

use crate::conductor::{Conductor, TickResult};
use crate::hyper_stigmergy::HyperStigmergicMorphogenesis;
use crate::rlm;

/// Runtime configuration for the main tick loop
pub struct LoopConfig {
    pub tick_interval: Duration,
    pub model: String,
    pub max_ticks: Option<u64>,
    pub auto_evolve_interval: u64,
    pub reflection_interval: u64,
}

impl Default for LoopConfig {
    fn default() -> Self {
        Self {
            tick_interval: Duration::from_secs(2),
            model: crate::ollama_client::resolve_model_from_env(
                "hf.co/DavidAU/OpenAi-GPT-oss-20b-HERETIC-uncensored-NEO-Imatrix-gguf:IQ4_NL",
            ),
            max_ticks: None,
            auto_evolve_interval: 10,
            reflection_interval: 3,
        }
    }
}

pub struct LoopRuntime {
    config: LoopConfig,
}

impl LoopRuntime {
    pub fn new() -> Self {
        Self {
            config: LoopConfig::default(),
        }
    }

    pub fn with_config(config: LoopConfig) -> Self {
        Self { config }
    }

    /// Run the main tick loop:
    /// Each tick executes the full Conductor cycle:
    ///   Prolog braids → skill retrieval → RLM execute → reflect → distill → evolve
    pub async fn run(&self) {
        let world = HyperStigmergicMorphogenesis::new(8);
        let mut conductor = Conductor::new(&self.config.model, world);

        let world_snapshot: HyperStigmergicMorphogenesis = conductor.world.read().await.clone();
        let mut rlm = rlm::rlm_from_world(world_snapshot, &self.config.model).await;

        let mut tick_count: u64 = 0;

        println!("╔══════════════════════════════════════════╗");
        println!("║  Hyper-Stigmergic Morphogenesis Runtime  ║");
        println!("║  Model: {:<32} ║", self.config.model);
        println!(
            "║  Tick interval: {:?}{} ║",
            self.config.tick_interval,
            " ".repeat(24 - format!("{:?}", self.config.tick_interval).len())
        );
        println!("╚══════════════════════════════════════════╝\n");

        loop {
            tick_count += 1;

            if let Some(max) = self.config.max_ticks {
                if tick_count > max {
                    println!("\nMax ticks ({}) reached. Shutting down.", max);
                    break;
                }
            }

            let result = conductor.tick(&mut rlm).await;

            self.print_tick_summary(&result);

            // Sync RLM world state back from conductor
            let world: HyperStigmergicMorphogenesis = conductor.world.read().await.clone();
            rlm.world = world;

            tokio::time::sleep(self.config.tick_interval).await;
        }
    }

    fn print_tick_summary(&self, result: &TickResult) {
        println!("── Tick {} ──────────────────────────────", result.tick);
        println!("  Intent: {}", result.intent);
        println!(
            "  Braids: {}/{} succeeded | Skills: {} applicable, {} distilled{}",
            result.synthesis.braids_succeeded,
            result.synthesis.braids_run,
            result.skills_applicable,
            result.skills_distilled,
            if result.skills_evolved {
                " | EVOLVED"
            } else {
                ""
            }
        );
        println!("  Reflection: {}", result.reflection.summary);
        if !result.exec_ok {
            println!("  ⚠ Execution failed");
        }
        println!();
    }
}
