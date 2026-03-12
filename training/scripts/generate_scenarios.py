#!/usr/bin/env python3
"""
Generate training scenarios for HSM-II from coding tasks.
Converts coding scenarios into agent configurations.
"""

import json
import random
import sys
from pathlib import Path
from typing import Dict, List, Any

def load_coding_scenarios(path: Path) -> Dict[str, Any]:
    """Load coding scenarios from JSON."""
    with open(path) as f:
        return json.load(f)

def generate_agent_config(task: Dict, agent_id: int) -> Dict[str, Any]:
    """Generate agent configuration for a task."""
    # Vary drives based on task type
    drive_variations = {
        "code_review": {"curiosity": 0.7, "harmony": 0.8, "growth": 0.5, "transcendence": 0.6},
        "refactoring": {"curiosity": 0.8, "harmony": 0.6, "growth": 0.8, "transcendence": 0.5},
        "architecture": {"curiosity": 0.9, "harmony": 0.4, "growth": 0.9, "transcendence": 0.8},
        "debugging": {"curiosity": 0.9, "harmony": 0.5, "growth": 0.6, "transcendence": 0.7},
        "optimization": {"curiosity": 0.8, "harmony": 0.5, "growth": 0.9, "transcendence": 0.6},
        "testing": {"curiosity": 0.7, "harmony": 0.7, "growth": 0.8, "transcendence": 0.5},
        "api_design": {"curiosity": 0.8, "harmony": 0.6, "growth": 0.8, "transcendence": 0.7},
        "documentation": {"curiosity": 0.6, "harmony": 0.9, "growth": 0.5, "transcendence": 0.6}
    }
    
    task_type = task.get("type", "general")
    base_drives = drive_variations.get(task_type, {"curiosity": 0.7, "harmony": 0.6, "growth": 0.7, "transcendence": 0.6})
    
    # Add some noise per agent
    noise = lambda: random.uniform(-0.1, 0.1)
    
    return {
        "id": f"agent_{agent_id:03d}",
        "role": random.choice(["Architect", "Catalyst", "Chronicler"]),
        "drives": {
            "curiosity": max(0.0, min(1.0, base_drives["curiosity"] + noise())),
            "harmony": max(0.0, min(1.0, base_drives["harmony"] + noise())),
            "growth": max(0.0, min(1.0, base_drives["growth"] + noise())),
            "transcendence": max(0.0, min(1.0, base_drives["transcendence"] + noise()))
        },
        "assigned_task": task["id"],
        "expertise": random.choice(["systems", "algorithms", "frontend", "backend", "devops"])
    }

def generate_scenario_config(tasks: List[Dict], num_agents: int) -> Dict[str, Any]:
    """Generate a complete scenario configuration."""
    # Assign tasks to agents (multiple agents can work on same task)
    agents = []
    for i in range(num_agents):
        task = random.choice(tasks)
        agent_config = generate_agent_config(task, i)
        agents.append(agent_config)
    
    # Generate hypergraph initial state
    initial_edges = []
    for i in range(num_agents):
        for j in range(i + 1, num_agents):
            # Connect agents with similar expertise or complementary skills
            if random.random() < 0.3:  # 30% edge density
                initial_edges.append({
                    "vertices": [i, j],
                    "weight": random.uniform(0.3, 0.9),
                    "tags": ["collaboration", random.choice(["mentorship", "peer_review", "pairing"])]
                })
    
    return {
        "scenario_type": "coding_collaboration",
        "agents": agents,
        "initial_edges": initial_edges,
        "tasks": tasks,
        "parameters": {
            "decay_rate": 0.01,
            "skill_evolution_interval": 200,
            "council_frequency_ticks": 50,
            "federation_sync_ticks": 10
        }
    }

def generate_training_configs(output_dir: Path, num_scenarios: int = 10):
    """Generate multiple scenario configurations for training."""
    scenarios_path = Path(__file__).parent.parent / "data" / "coding_scenarios.json"
    
    if not scenarios_path.exists():
        print(f"Scenarios file not found: {scenarios_path}")
        sys.exit(1)
    
    data = load_coding_scenarios(scenarios_path)
    tasks = data["tasks"]
    
    output_dir.mkdir(parents=True, exist_ok=True)
    
    for i in range(num_scenarios):
        # Randomize agent count per scenario
        num_agents = random.randint(10, 20)
        
        config = generate_scenario_config(tasks, num_agents)
        config["scenario_id"] = f"coding_scenario_{i:03d}"
        config["seed"] = 1000 + i
        
        output_file = output_dir / f"scenario_{i:03d}.json"
        with open(output_file, 'w') as f:
            json.dump(config, f, indent=2)
        
        print(f"Generated: {output_file}")
    
    print(f"\nGenerated {num_scenarios} scenario configurations in {output_dir}")

if __name__ == '__main__':
    output_dir = Path("training/data/scenarios")
    num_scenarios = int(sys.argv[1]) if len(sys.argv) > 1 else 10
    
    generate_training_configs(output_dir, num_scenarios)
