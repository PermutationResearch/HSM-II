pub mod action;
pub mod agent;
pub mod analysis;
pub mod clean;
pub mod columnar_engine;
pub mod conductor;
pub mod consensus;
pub mod cypher_parser;
pub mod database;
pub mod disk_backed_vector_index;
pub mod dspy;
pub mod dspy_session;
pub mod embedded_graph_store;
pub mod embedding_index;
pub mod experiment;
pub mod external_connectors;
pub mod federation;
pub mod hyper_stigmergy;
pub mod hypergraph;
pub mod kuramoto;
pub mod loop_main;
pub mod memory;
pub mod meta_graph;
pub mod optimizer;
pub mod prolog_embedding_bridge;
pub mod prolog_engine;
pub mod property_graph;
pub mod query_engine;
pub mod reasoning_braid;
pub mod rlm;
pub mod rlm_v2;
pub mod skill;
pub mod transaction_layer;
pub mod workflow;
pub mod world_controller;

// New modules
pub mod agent_core;
pub mod cass;
pub mod coder_assistant;
pub mod communication;
pub mod council;
pub mod dks;
pub mod email;
pub mod gpu;
pub mod graph_runtime;
pub mod hnsw_index;
pub mod lcm;
pub mod llm;
pub mod navigation;
pub mod ollama_client;
pub mod ouroboros_compat;
pub mod pi_ai_compat;
pub mod pi_tools;
pub mod real;
pub mod reward;
pub mod social_memory;
pub mod stigmergic_policy;
pub mod vault;

// Personal Agent module (Hermes-like grounded interface)
pub mod personal;

// Feature flags for anti-fragile agent deployment
pub mod flags;

// Tool system (Rust-native, async)
pub mod tools;

// Production observability
pub mod observability;

// Authentication & Authorization
pub mod auth;

// Platform gateways
pub mod gateways;

// Job scheduler for cron and background tasks
pub mod scheduler;

// MiroFish-inspired scenario simulation engine
pub mod scenario_simulator;

// Codex-style TUI - Dark minimalist terminal aesthetic
pub mod tui_codex_style;

// Demo tests (only in test mode)
#[cfg(test)]
pub mod social_memory_demo;

pub use action::Action;
pub use agent::{Agent, AgentId, Drives, Role};
pub use analysis::{DensityMetrics, HypergraphAnalysis};
pub use columnar_engine::{ColumnarGraphStore, ColumnarTable};
pub use conductor::{Conductor, FederationTickResult, TickResult, UiEvent};
pub use consensus::{
    AssociationType, BayesianConfidence, ConsensusEngine, ConsensusResult, ConsensusVerdict,
    ContextPolicy, CorrelationMonitor, DyadResult, EmergentAssociation, GuardianCritic,
    IdentityBridgeRegularizer, JuryContext, JuryLayer, JuryPipeline, RespecAction, SkillStatus,
    VetoCheck,
};
pub use cypher_parser::{
    CypherParser, CypherQuery, MatchClause, MatchNodePattern, MatchRelationshipPattern, ReturnExpr,
    WhereClause,
};
pub use embedding_index::InMemoryEmbeddingIndex;
pub use experiment::{ExperimentConfig, ExperimentHarness, ExperimentStats};
pub use federation::{
    ConflictMediator, ConflictRecord, ConflictResolution, CrossSystemVote, EdgeScope,
    FederationClient, FederationConfig, FederationServer, HyperedgeInjectionRequest, ImportResult,
    InjectedEdge, KnowledgeLayer, MetaHyperedge, PromotedEdge, Provenance, SharedEdge,
    SharedVertexMeta, Subscription, SubscriptionFilter, SystemId, SystemInfo, TrustEdge,
    TrustGraph, TrustPolicy,
};
pub use hyper_stigmergy::HyperStigmergicMorphogenesis;
pub use hypergraph::{Hypergraph, HypergraphConv};
pub use kuramoto::{
    build_adjacency as kuramoto_build_adjacency, confidence_to_phase, KuramotoConfig,
    KuramotoDiagnostics, KuramotoEngine, KuramotoSnapshot, OscillatorSnapshot,
};
pub use loop_main::{LoopConfig, LoopRuntime};
pub use memory::{
    default_tool_registry, AgentTool, HybridMemory, MemoryEntry, MemoryNetwork, MemoryStats,
    RecallResult, StrategyScores, ToolContext, ToolRegistry, ToolResult, ToolSideEffect,
};
pub use meta_graph::MetaGraph;
pub use optimizer::{Assignment, AssignmentMetrics, TaskAssignmentOptimizer, TaskRequirements};
pub use prolog_embedding_bridge::{
    EmbeddedFact, EmbeddingAwareProlog, FactCategory, FactEmbeddingIndex, NeuralQueryResult,
    NeuralSymbolicBraid, PrologEmbeddingBridge,
};
pub use prolog_engine::{Atom, PrologEngine, QueryResult, Rule, Term};
pub use property_graph::{
    GraphNodeRecord, GraphRelationshipRecord, PropertyGraphSnapshot, PropertyValue,
};
pub use query_engine::{CypherEngine, QueryResultSet};
pub use real::{
    AgentSnapshot, ApplyActionRequest, ApplyActionResponse, BidSubmission, DecisionResult,
    GrpoReward, GrpoUpdateRequest, Objectives, TickResponse, WorldSnapshot,
};
pub use reasoning_braid::{BraidOrchestrator, BraidStatus, BraidSynthesis, ReasoningBraid};
pub use reward::{
    DatasetTaskEvaluator, DefaultTaskEvaluator, RewardWeights, TaskEvalContext, TaskEvaluator,
    WeightedTaskEvaluator,
};
pub use rlm::{
    rlm_from_world, BidConfig, Context, EmbeddingCache, LivingPrompt, OllamaHandle, RlmAction,
    RlmMessage, SelfImprovementCycle, SubAgent, RLM,
};
pub use rlm_v2::{
    run_rlm, Context as RlmV2Context, ContextChunk, ContextMetadata, ExecutionResult, FinalAnswer,
    LlmBridge, LlmBridgeConfig, LlmQuery, RlmConfig, RlmError as RlmV2Error, RlmExecutor,
    RlmIteration, RlmRuntime, RlmStats, RlmStatus, SandboxConfig, SubQuery, SubQueryResponse,
    Trajectory, TrajectoryStore, TrajectoryViewer, IterationSnapshot, RlmToolCall,
};
pub use skill::{ApplicabilityCondition, Skill, SkillBank, SkillLevel, SkillSource};
pub use social_memory::{
    AgentReputation, CapabilityEvidence, CollaborationStats, DataSensitivity, DelegationCandidate,
    DelegationScoreComponents, PromiseRecord, PromiseStatus, SharePolicy, SocialMemory,
};

// Feature flags for progressive rollout
pub use flags::{
    EvaluationContext, FeatureFlag, FlagMetadata, FlagStats, FlagStore, FlagsAware, Operator,
    TargetingRule,
};
pub use stigmergic_policy::{
    PolicyShift, RoutingDirective, StigmergicMemory as RuntimeStigmergicMemory, StigmergicTrace,
    TraceKind,
};
pub use transaction_layer::{Transaction, TransactionManager};
pub use workflow::{
    ContextValue, StepResult, Workflow, WorkflowBuilder, WorkflowContext, WorkflowRegistry,
};
pub use world_controller::WorldController;

// Re-export new modules
pub use council::{
    debate::{Argument, DebateCouncil, DebateRound},
    llm_deliberation::{DebatePhase, LLMArgument, LLMDebateCouncil, LLMDeliberationConfig, Stance},
    orchestrate::{Command, OrchestratorCouncil, SubTask},
    ralph::{AgentConfig, RalphConfig, RalphCouncil, RalphIteration, RalphState, RalphVerdict},
    simple::{SimpleCouncil, Vote},
    Council, CouncilDecision, CouncilDecisionMetadata, CouncilEvidence, CouncilEvidenceKind,
    CouncilFactory, CouncilGraphQuery, CouncilId, CouncilMember, CouncilMode, CouncilStatus,
    Decision, ExecutionPlan, ModeConfig, ModeScoreBreakdown, ModeSelectionReport, ModeSwitchEvent,
    ModeSwitcher, Proposal, StigmergicCouncilContext,
};

pub use dks::{
    calculate_dks_stability,
    flux::{Environment, Flux, FluxType},
    multifractal::{compositionality_measure, MultifractalSpectrum, MultiscaleDKS},
    population::{EvolutionParameters, Population, PopulationStats},
    replicator::{Metabolism, Replicator, ReplicatorState, Resource},
    selection::{PersistenceMeasure, SelectionEvent, SelectionPressure},
    // Stigmergic DKS - ecological dynamics integration
    stigmergic_entity::{
        CognitiveState, FieldReading, StigmergicAction, StigmergicDKS, StigmergicEdgeType,
        StigmergicEntity, StigmergicMemory, StigmergicPattern, StigmergicPopulation,
        StigmergicStats, StigmergicTickResult,
    },
    DKSConfig,
    DKSSystem,
    DKSTickResult,
};

pub use cass::{
    context::{ContextManager, ContextSnapshot, RelevanceScorer},
    embedding::{EmbeddingEngine, SimilarityMetric, SkillEmbedding},
    semantic_graph::{EdgeType, SemanticGraph, SkillNode},
    RelatedSkill, SemanticSkillMatch, SkillChain, CASS,
};

pub use navigation::{
    indexer::{CodeIndex, SemanticIndex, TopicModel},
    parser::{CodeParser, Language, ParsedUnit, UnitType},
    search::{QueryIntent, SearchResult, SemanticSearch},
    CodeNavigator, IndexStats,
};

pub use communication::{
    gossip::{GossipConfig, GossipProtocol, RumorState, RumorStatus},
    message::{Message, MessageMetadata, MessageType},
    protocol::{DeliveryGuarantee, MessageEnvelope, MessagePriority, RoutingStrategy},
    swarm::{FlockingForces, StigmergicField, SwarmCommunication, WaggleDance},
    CommunicationConfig, CommunicationHub, FieldType, GossipStats, Position, Target,
};

// GPU-accelerated graph processing
pub use gpu::{
    graph::{ForceDirectedLayout, GraphLayout, HierarchicalLayout, SpectralLayout},
    CpuFallback, GpuAccelerator, GraphOperation,
};

#[cfg(feature = "gpu")]
pub use gpu::{
    buffer::{BufferPool, GpuBuffer},
    compute::{ComputeShader, GpuCompute, ShaderKernel},
    graph::GpuGraph,
};

// Local LLM inference (FrankenTorch-style)
pub use llm::{
    cache::{CacheManager, KvCache, SlidingWindowCache},
    engine::{GenerationParams, InferenceConfig, LlmEngine},
    model::{ModelInfo, ModelLoader, ModelType, ModelWeights, Quantization},
    tokenizer::{ChatMessage, ChatRole, ChatTemplate, EncodingOptions, TokenEncoder},
    CodeAnalysis, DistilledSkill, FrankenConfig, FrankenTorch, InferenceRequest, ModelServer,
    RequestPriority,
};

// Email agent integration
pub use email::{
    classifier::{Category, Classification, EmailClassifier, Priority},
    client::{EmailClient, EmailProvider, ImapConfig, SmtpConfig},
    memory::{ConversationThread, EmailMemory},
    responder::{QuickReplyType, ResponseGenerator, ResponseTemplate, Tone},
    Attachment, Email, EmailAction, EmailAgent, EmailConfig, EmailStats, OutgoingEmail,
};

pub use database::{
    CodeAgentMessageRow, CodeAgentSessionRow, CodeAgentToolCallRow, CouncilClaimRow,
    DspyDemonstrationRow, DspyOptimizedConfigRow, DspyTraceRow, MessageRow, OuroborosGateAuditRow,
    OuroborosMemoryEventRow, RewardLogRow, RooDb, RooDbConfig, SkillEvidenceRow, SkillRow,
    VaultEmbeddingRow,
};

pub use disk_backed_vector_index::DiskBackedVectorIndex;
pub use dspy::{
    bootstrap_demonstrations, optimize_all_signatures, optimize_signature, persist_trace,
    run_signature, run_signature_traced, Demonstration, DspyContext, DspySignature,
    OptimizationResult, ResolvedSignature, SignatureStore, TraceResult,
};
pub use dspy_session::{
    DspySession, DspySessionAdapter, OptimizationExample, SessionConfig, SessionSnapshot,
    SessionTurn, TurnRole,
};
pub use embedded_graph_store::{
    EmbeddedGraphStore, EmbeddedGraphStoreSnapshot, EMBEDDED_GRAPH_STORE_FILE,
    EMBEDDED_GRAPH_WAL_FILE, LEGACY_EMBEDDING_INDEX_FILE, LEGACY_WORLD_STATE_FILE,
};
pub use external_connectors::{
    DuckDbCliConnector, ExternalConnector, ExternalRow, ExternalTable, JsonArrayConnector,
    PostgresCliConnector,
};
pub use graph_runtime::{GraphActionPlan, GraphActionResult, GraphRuntime, GraphToolKind};
pub use hnsw_index::HnswLikeIndex;

pub use ouroboros_compat::{
    phase1_policy::{
        ConstitutionConfig, PolicyContext, PolicyDecision, PolicyEngine, PolicyVerdict,
        ReleaseState,
    },
    phase2_risk_gate::{RiskAssessment, RiskGate, RiskGateConfig, RiskLevel},
    phase3_council_bridge::{CouncilBridge, CouncilBridgeConfig, CouncilGatePlan},
    phase4_evidence_contract::{
        EvidenceBundle, EvidenceContract, EvidenceRequirements, EvidenceValidation,
    },
    phase5_ops_memory::{
        default_trust_policy, evaluate_full_mesh, evaluate_runtime_slos, EventSourcedMemory,
        ExportCadence, ExportScheduler, MemoryEvent, MemoryEventKind, MeshHealth,
        MutableMemoryCache, RuntimeSloReport, RuntimeSnapshot, RuntimeThresholds,
    },
    ActionKind as OuroborosActionKind, ProposedAction,
};

// Pi Agent coding tools integration
pub use pi_tools::{
    create_pi_tools, PiBashTool, PiEditTool, PiFindTool, PiGrepTool, PiLsTool, PiReadTool,
    PiWriteTool,
};

// Coder Assistant integration (streaming, tool calling, differential rendering)
pub use coder_assistant::{
    now, AgentLoop, DifferentialRenderer, MarkdownRenderer, Message as CoderMessage, MessageRole,
    ProviderConfig, RenderUpdate, SandboxMode, SecretBoundary, Session, SessionEvent,
    SessionManager, StreamEvent, StreamingHandler, ThinkingBlock, ToolCall,
    ToolContext as CoderToolContext, ToolExecutionAudit, ToolExecutionPolicy, ToolExecutionResult,
    ToolExecutor, ToolParameter, ToolProviderKind, ToolProviderMetadata, ToolProviderRuntime,
    ToolRegistry as CoderToolRegistry, ToolResult as CoderToolResult, ToolSchema, WasmCapability,
};

// Pi-AI Compatible API (JavaScript-style API for Ollama)
pub use pi_ai_compat::{
    complete, complete_streaming, getModel, prelude as pi_ai_prelude, CompleteOptions,
    Context as PiContext, Message as PiMessage, Model as PiModel, PiAiError,
    Response as PiResponse, Role as PiRole, ToolDef as PiToolDef, Usage as PiUsage,
};

// AutoContext — Closed-loop learning (Competitor → Analyst → Coach → Curator)
pub mod autocontext;
pub use autocontext::{
    AutoContextLoop, AutoContextStore, DistillationRouter, FrontierConfig, HarnessResult, Hint,
    KnowledgeBase, LoopConfig as AutoContextLoopConfig, LoopResult, ModelTier, Playbook,
    PlaybookHarness, RetrievedContext, ScenarioBuilder, StorageConfig, TrainingExample,
    ValidationPipeline, ValidationResult, ValidationStage,
};

// Stigmergic Dream Consolidation — offline experience replay + temporal pattern learning
pub mod dream;
pub use dream::{
    CrystallizedPattern, DreamConfig, DreamCycleResult, ProtoSkill, StigmergicDreamEngine,
    TemporalMotif,
};

// optimize_anything
pub mod optimize_anything;
pub use optimize_anything::{
    evaluate_synthesis, session_from_json, Artifact, Candidate, EvalResult, Evaluator,
    KeywordEvaluator, LlmJudgeEvaluator, OptimizationConfig, OptimizationMode, OptimizationSession,
    ASI,
};

// Agent Core (pi-agent-core style architecture)
pub use agent_core::{
    agent::AgentBuilder as NewAgentBuilder, events::EventBus,
    loop_core::AgentError as NewAgentError, now as agent_now, transport::TransportError,
    Agent as NewAgent, AgentEvent, AgentLoop as NewAgentLoop, AgentState as NewAgentState,
    Attachment as NewAttachment, AttachmentType as NewAttachmentType, DirectTransport,
    EventHandler, Message as AgentMessage, MessageQueue, ModelConfig, ProxyTransport, QueueMode,
    Role as AgentRole, Tool as NewTool, ToolCall as NewToolCall, ToolError as NewToolError,
    ToolHandler as NewToolHandler, Transport,
};

// Re-export from hyper_stigmergy for backward compatibility
pub use hyper_stigmergy::{BeliefSource, ExperienceOutcome};

/// Re-export common types for convenience
pub type Result<T> = anyhow::Result<T>;

// ═════════════════════════════════════════════════════════════════════════════
// EMPIRICAL EVALUATION MODULES (for paper experiments)
// ═════════════════════════════════════════════════════════════════════════════

pub mod batch_runner;
pub mod metrics;
pub mod metrics_dks_ext;

// Investigation Agent System
pub mod investigation_engine;
pub mod investigation_tools;
pub use investigation_engine::{
    DatasetInfo, EngineEvent, EntityInfo, EvidenceChainInfo, FindingInfo, FindingSeverity,
    InvestigationConfig, InvestigationEngine, InvestigationSession, InvestigationSummary,
    SessionId, SessionStatus, SubtaskInfo, SubtaskStatus,
};
pub use investigation_tools::{InvestigationToolRegistry, ToolCallRecord};

// MiroFish Trajectory Engine — full trajectory mechanics for business decisions
pub mod mirofish;
pub use mirofish::{
    builtin_templates, compute_calibration, recalibrate_confidence, validate_variables,
    AnalysisComparison, CalibrationStats, FlowState, FlowTransition, LlmBackend,
    LlmBackendResult, MiroFishEngine, PredictionRecord, PredictionStore,
    ProbabilityFlowNetwork, ProjectionCurve, ProjectionPoint, RefinementRound,
    RefinementSession, ScenarioDomain, ScenarioTemplate, StoredAnalysis,
    Trajectory as MiroFishTrajectory, TrajectoryAnalysis, TrajectoryStep,
    ValidationResult as MiroFishValidation, VariableSpec, VariableType,
};

// Autonomous Business Team — role-based agent personas, channel connectors, campaign feedback
pub mod autonomous_team;
pub use autonomous_team::{
    build_persona as build_team_persona, BrandContext, BrandVoice, BusinessRole, Campaign,
    CampaignSnapshot, CampaignStatus, CampaignStore, ChannelConnector, ChannelPerformanceSummary,
    ChannelType, CommunitySignal, ContentMetric, ContentPiece, DomainPerformance,
    LocalFileConnector, MemberStatus, PublishResult, RoleIntent, SignalType, TeamMember,
    TeamOrchestrator,
};

// Multi-Tenant SaaS Layer — tenant isolation, team API, usage tracking
pub mod tenant;
pub mod team_api;
pub mod usage_tracker;

// Dream → Routing Feedback Loop — converts dream patterns into task routing adjustments
pub mod dream_advisor;
pub use dream_advisor::DreamAdvisor;

// Onboarding, Belief Extraction, and Document Ingestion
pub mod onboard;
pub use onboard::{
    extract_beliefs_from_chat, ingest_file, post_chat_extract_and_store, run_onboard_batch,
    run_onboard_interactive, store_extracted_beliefs, ExtractedBelief, IngestConfig, IngestResult,
    OnboardResult,
};

pub use metrics::{
    AggregatedStats, BatchAggregator, ExperimentRun, FederationEvent, MetricsCollector,
    MetricsCouncilDecision, MetricsExperimentConfig, TickSnapshot,
};

pub use batch_runner::{BatchConfig, BatchRunner};

pub use metrics_dks_ext::{DKSMetrics, TrustGraphMetrics};
