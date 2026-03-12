"""
Hermes Extension Server for HSM-II Integration

This FastAPI server extends [Hermes Agent](https://github.com/NousResearch/hermes-agent) with endpoints for HSM-II communication,
enabling the Rust-based HSM-II system to leverage Hermes's tool ecosystem.
"""

import asyncio
import json
import logging
import os
from contextlib import asynccontextmanager
from datetime import datetime
from typing import Any, Dict, List, Optional
from uuid import uuid4

from fastapi import FastAPI, HTTPException, BackgroundTasks
from fastapi.middleware.cors import CORSMiddleware
from pydantic import BaseModel, Field

# Configure logging
logging.basicConfig(level=logging.INFO)
logger = logging.getLogger(__name__)

# Try to import Hermes modules
# Note: This assumes Hermes is installed or in PYTHONPATH
try:
    from run_agent import AIAgent
    from tools.registry import registry
    HERMES_AVAILABLE = True
except ImportError:
    logger.warning("Hermes modules not available. Running in mock mode.")
    HERMES_AVAILABLE = False


# ============================================================================
# Pydantic Models
# ============================================================================

class ToolCall(BaseModel):
    name: str
    arguments: Dict[str, Any]
    result: Optional[Dict[str, Any]] = None


class Turn(BaseModel):
    turn_number: int
    role: str
    content: str
    tool_calls: Optional[List[ToolCall]] = None


class HSMIIContext(BaseModel):
    memory: Dict[str, str] = Field(default_factory=dict)
    user_profile: Dict[str, Any] = Field(default_factory=dict)
    hsmii_state: Dict[str, Any] = Field(default_factory=dict)


class ExecuteRequest(BaseModel):
    task_id: str = Field(default_factory=lambda: str(uuid4()))
    prompt: str
    toolsets: List[str] = Field(default_factory=lambda: ["web", "terminal"])
    max_turns: int = 20
    context: Optional[HSMIIContext] = None
    system_prompt: Optional[str] = None


class ExecuteResponse(BaseModel):
    task_id: str
    result: str
    tool_calls: List[ToolCall] = Field(default_factory=list)
    trajectory: List[Turn] = Field(default_factory=list)
    status: str  # "success", "partial_success", "failed", "timeout", "cancelled"
    metadata: Optional[Dict[str, Any]] = None


class HermesSkill(BaseModel):
    name: str
    description: str
    tags: List[str] = Field(default_factory=list)
    content: str
    source: Optional[str] = None
    metadata: Optional[Dict[str, Any]] = None


class SkillConflict(BaseModel):
    skill_name: str
    reason: str
    hermes_version: Optional[HermesSkill] = None
    hsmii_version: Optional[HermesSkill] = None


class SkillSyncResult(BaseModel):
    imported: List[HermesSkill] = Field(default_factory=list)
    exported: List[HermesSkill] = Field(default_factory=list)
    conflicts: List[SkillConflict] = Field(default_factory=list)


class HealthResponse(BaseModel):
    status: str
    version: str = "0.1.0"
    uptime_seconds: int
    available_toolsets: List[str]
    active_sessions: int


# ============================================================================
# Global State
# ============================================================================

class ServerState:
    def __init__(self):
        self.start_time = datetime.now()
        self.active_agents: Dict[str, AIAgent] = {}
        self.session_count = 0
        
    def get_uptime(self) -> int:
        return int((datetime.now() - self.start_time).total_seconds())

state = ServerState()


# ============================================================================
# FastAPI App
# ============================================================================

@asynccontextmanager
async def lifespan(app: FastAPI):
    """Application lifespan handler"""
    logger.info("Starting HSM-II Hermes Extension Server...")
    
    if HERMES_AVAILABLE:
        logger.info("Hermes modules loaded successfully")
        available = registry.get_available_toolsets()
        logger.info(f"Available toolsets: {available}")
    else:
        logger.warning("Running in MOCK mode - Hermes tools unavailable")
    
    yield
    
    logger.info("Shutting down...")


app = FastAPI(
    title="HSM-II Hermes Extension",
    description="Bridge API between HSM-II and Hermes Agent",
    version="0.1.0",
    lifespan=lifespan
)

app.add_middleware(
    CORSMiddleware,
    allow_origins=["*"],
    allow_credentials=True,
    allow_methods=["*"],
    allow_headers=["*"],
)


# ============================================================================
# Routes
# ============================================================================

@app.get("/")
async def root():
    return {
        "service": "HSM-II Hermes Extension",
        "version": "0.1.0",
        "hermes_available": HERMES_AVAILABLE
    }


@app.get("/api/v1/health", response_model=HealthResponse)
async def health():
    """Health check endpoint"""
    available_toolsets = []
    
    if HERMES_AVAILABLE:
        available_toolsets = list(registry.get_available_toolsets().keys())
    else:
        # Mock toolsets for testing
        available_toolsets = ["web", "terminal", "skills", "memory", "todo"]
    
    return HealthResponse(
        status="healthy" if HERMES_AVAILABLE else "degraded",
        uptime_seconds=state.get_uptime(),
        available_toolsets=available_toolsets,
        active_sessions=len(state.active_agents)
    )


@app.get("/api/v1/toolsets")
async def get_toolsets():
    """Get available toolsets"""
    if HERMES_AVAILABLE:
        return list(registry.get_available_toolsets().keys())
    return ["web", "terminal", "skills", "memory", "todo", "cron", "delegation"]


@app.post("/api/v1/execute", response_model=ExecuteResponse)
async def execute(request: ExecuteRequest):
    """Execute a task via Hermes Agent"""
    logger.info(f"Executing task {request.task_id}: {request.prompt[:50]}...")
    
    if not HERMES_AVAILABLE:
        # Mock execution for testing
        return ExecuteResponse(
            task_id=request.task_id,
            result=f"[MOCK] Executed: {request.prompt}",
            tool_calls=[],
            trajectory=[
                Turn(turn_number=1, role="user", content=request.prompt),
                Turn(turn_number=2, role="assistant", content=f"[MOCK] Result for: {request.prompt}")
            ],
            status="success",
            metadata={"mock": True}
        )
    
    try:
        # Initialize agent
        agent = AIAgent(
            model=os.getenv("HERMES_MODEL", "anthropic/claude-opus-4"),
            enabled_toolsets=request.toolsets,
            max_turns=request.max_turns,
        )
        
        # Set up system prompt if provided
        if request.system_prompt:
            # Note: This assumes AIAgent supports system prompt customization
            pass
        
        # Load HSM-II context into memory
        if request.context:
            if request.context.memory:
                # Update agent memory with HSM-II context
                for key, value in request.context.memory.items():
                    # Add to MEMORY.md equivalent
                    pass
        
        # Execute task
        result = agent.chat(request.prompt)
        
        # Extract trajectory/tool calls if available
        trajectory = []
        tool_calls = []
        
        if hasattr(agent, 'conversation_history'):
            for i, turn in enumerate(agent.conversation_history):
                trajectory.append(Turn(
                    turn_number=i + 1,
                    role=turn.get('role', 'unknown'),
                    content=turn.get('content', ''),
                ))
        
        # Determine status
        status = "success"  # Simplified - could analyze result for actual status
        
        return ExecuteResponse(
            task_id=request.task_id,
            result=result,
            tool_calls=tool_calls,
            trajectory=trajectory,
            status=status,
            metadata={
                "model": agent.model if hasattr(agent, 'model') else "unknown",
                "toolsets_used": request.toolsets
            }
        )
        
    except Exception as e:
        logger.error(f"Execution failed: {e}")
        raise HTTPException(status_code=500, detail=str(e))


@app.post("/api/v1/skills/sync", response_model=SkillSyncResult)
async def sync_skills(skills: List[HermesSkill]):
    """Bidirectional skill synchronization"""
    logger.info(f"Syncing {len(skills)} skills from HSM-II")
    
    result = SkillSyncResult()
    
    # Import skills from HSM-II
    for skill in skills:
        try:
            # Save to Hermes skills directory
            skill_path = os.path.expanduser(f"~/.hermes/skills/hsmii_{skill.name}.md")
            os.makedirs(os.path.dirname(skill_path), exist_ok=True)
            
            with open(skill_path, 'w') as f:
                f.write(skill.content)
            
            result.imported.append(skill)
            logger.info(f"Imported skill: {skill.name}")
            
        except Exception as e:
            result.conflicts.append(SkillConflict(
                skill_name=skill.name,
                reason=f"Import failed: {str(e)}"
            ))
    
    # Export Hermes skills to HSM-II
    if HERMES_AVAILABLE:
        try:
            # Get Hermes skills
            hermes_skills_dir = os.path.expanduser("~/.hermes/skills")
            if os.path.exists(hermes_skills_dir):
                for filename in os.listdir(hermes_skills_dir):
                    if filename.endswith('.md') and not filename.startswith('hsmii_'):
                        filepath = os.path.join(hermes_skills_dir, filename)
                        with open(filepath, 'r') as f:
                            content = f.read()
                        
                        skill = HermesSkill(
                            name=filename.replace('.md', ''),
                            description="Hermes skill",
                            content=content,
                            source="hermes"
                        )
                        result.exported.append(skill)
        except Exception as e:
            logger.error(f"Export failed: {e}")
    
    return result


@app.post("/api/v1/federation/message")
async def federation_message(message: dict):
    """Receive a federation message from HSM-II"""
    logger.info(f"Received federation message: {message.get('message_id')}")
    
    # Process stigmergic signal
    signal = message.get('signal', {})
    signal_type = signal.get('signal_type', 'unknown')
    
    # Route to appropriate gateway (Discord, Telegram, etc.)
    # This would integrate with Hermes's gateway system
    
    return {"status": "received", "signal_type": signal_type}


@app.get("/api/v1/agents/active")
async def get_active_agents():
    """Get list of active subagents"""
    return {
        "active_count": len(state.active_agents),
        "agents": [
            {"id": aid, "status": "running"}
            for aid in state.active_agents.keys()
        ]
    }


# ============================================================================
# Startup
# ============================================================================

if __name__ == "__main__":
    import uvicorn
    
    port = int(os.getenv("HERMES_EXTENSION_PORT", "8000"))
    host = os.getenv("HERMES_EXTENSION_HOST", "0.0.0.0")
    
    logger.info(f"Starting server on {host}:{port}")
    uvicorn.run(app, host=host, port=port)
