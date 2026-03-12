#!/usr/bin/env python3
"""
HSM-II Training Results Analyzer
Analyzes training runs and generates comprehensive reports.
"""

import json
import sys
import os
from pathlib import Path
from typing import Dict, List, Any
import numpy as np
import pandas as pd
import matplotlib.pyplot as plt
import seaborn as sns

def load_run_data(run_dir: Path) -> Dict[str, Any]:
    """Load all data from a training run directory."""
    data = {
        'snapshots': [],
        'council': [],
        'federation': [],
        'summaries': []
    }
    
    # Find all run subdirectories
    for run_path in run_dir.glob('run_*'):
        if run_path.is_dir():
            # Load snapshots
            snap_file = list(run_path.glob('*snapshots.csv'))
            if snap_file:
                df = pd.read_csv(snap_file[0])
                data['snapshots'].append(df)
            
            # Load council decisions
            council_file = list(run_path.glob('*council.csv'))
            if council_file:
                df = pd.read_csv(council_file[0])
                data['council'].append(df)
            
            # Load summaries
            summary_file = list(run_path.glob('*summary.json'))
            if summary_file:
                with open(summary_file[0]) as f:
                    data['summaries'].append(json.load(f))
    
    return data

def analyze_coherence(snapshots: List[pd.DataFrame]) -> Dict[str, Any]:
    """Analyze coherence growth across runs."""
    if not snapshots:
        return {}
    
    final_coherences = [df['global_coherence'].iloc[-1] for df in snapshots]
    growths = [df['global_coherence'].iloc[-1] - df['global_coherence'].iloc[0] 
               for df in snapshots]
    
    return {
        'final_coherence_mean': np.mean(final_coherences),
        'final_coherence_std': np.std(final_coherences),
        'growth_mean': np.mean(growths),
        'growth_std': np.std(growths),
        'best_run': int(np.argmax(final_coherences)),
        'worst_run': int(np.argmin(final_coherences))
    }

def analyze_council(council_dfs: List[pd.DataFrame]) -> Dict[str, Any]:
    """Analyze council decision patterns."""
    if not council_dfs:
        return {}
    
    all_decisions = pd.concat(council_dfs, ignore_index=True)
    
    # Mode usage
    mode_counts = all_decisions['mode'].value_counts()
    
    # Outcome distribution
    outcome_counts = all_decisions['outcome'].value_counts()
    total = len(all_decisions)
    
    # Complexity vs Urgency correlation
    complexity_urgency_corr = all_decisions['complexity'].corr(all_decisions['urgency'])
    
    return {
        'total_decisions': total,
        'mode_distribution': mode_counts.to_dict(),
        'outcome_rates': {k: v/total for k, v in outcome_counts.items()},
        'complexity_urgency_correlation': complexity_urgency_corr,
        'avg_complexity': all_decisions['complexity'].mean(),
        'avg_urgency': all_decisions['urgency'].mean()
    }

def analyze_skills(snapshots: List[pd.DataFrame]) -> Dict[str, Any]:
    """Analyze skill distillation metrics."""
    if not snapshots:
        return {}
    
    final_skills = [df['skills_promoted'].iloc[-1] for df in snapshots]
    pass_rates = [df['jury_pass_rate'].iloc[-1] for df in snapshots]
    
    return {
        'skills_promoted_mean': np.mean(final_skills),
        'skills_promoted_std': np.std(final_skills),
        'jury_pass_rate_mean': np.mean(pass_rates),
        'jury_pass_rate_std': np.std(pass_rates)
    }

def plot_coherence_trajectories(snapshots: List[pd.DataFrame], output_dir: Path):
    """Plot coherence trajectories for all runs."""
    plt.figure(figsize=(12, 6))
    
    for i, df in enumerate(snapshots):
        plt.plot(df['tick'], df['global_coherence'], alpha=0.3, color='blue')
    
    # Plot mean trajectory
    if snapshots:
        max_len = max(len(df) for df in snapshots)
        mean_coherence = []
        ticks = range(max_len)
        
        for t in ticks:
            values = [df['global_coherence'].iloc[t] for df in snapshots 
                     if t < len(df)]
            mean_coherence.append(np.mean(values))
        
        plt.plot(ticks, mean_coherence, 'r-', linewidth=2, label='Mean')
    
    plt.xlabel('Tick')
    plt.ylabel('Global Coherence')
    plt.title('Coherence Trajectories Across Runs')
    plt.legend()
    plt.grid(True, alpha=0.3)
    plt.savefig(output_dir / 'coherence_trajectories.png', dpi=150)
    plt.close()

def plot_mode_distribution(council_dfs: List[pd.DataFrame], output_dir: Path):
    """Plot council mode distribution."""
    if not council_dfs:
        return
    
    all_decisions = pd.concat(council_dfs, ignore_index=True)
    mode_counts = all_decisions['mode'].value_counts()
    
    plt.figure(figsize=(10, 6))
    mode_counts.plot(kind='bar', color=['#2ecc71', '#3498db', '#e74c3c'])
    plt.xlabel('Council Mode')
    plt.ylabel('Number of Decisions')
    plt.title('Council Mode Distribution')
    plt.xticks(rotation=0)
    plt.tight_layout()
    plt.savefig(output_dir / 'mode_distribution.png', dpi=150)
    plt.close()

def generate_report(run_dir: Path, output_dir: Path):
    """Generate comprehensive analysis report."""
    print(f"Analyzing run: {run_dir}")
    
    data = load_run_data(run_dir)
    
    # Perform analyses
    coherence_stats = analyze_coherence(data['snapshots'])
    council_stats = analyze_council(data['council'])
    skill_stats = analyze_skills(data['snapshots'])
    
    # Create report
    report = {
        'run_directory': str(run_dir),
        'total_runs': len(data['snapshots']),
        'coherence': coherence_stats,
        'council': council_stats,
        'skills': skill_stats
    }
    
    # Save JSON report
    with open(output_dir / 'analysis_report.json', 'w') as f:
        json.dump(report, f, indent=2)
    
    # Generate plots
    plot_coherence_trajectories(data['snapshots'], output_dir)
    plot_mode_distribution(data['council'], output_dir)
    
    # Print summary
    print("\n" + "="*60)
    print("TRAINING ANALYSIS SUMMARY")
    print("="*60)
    print(f"\nTotal Runs: {report['total_runs']}")
    
    if coherence_stats:
        print(f"\n📊 Coherence:")
        print(f"   Final: {coherence_stats['final_coherence_mean']:.3f} ± {coherence_stats['final_coherence_std']:.3f}")
        print(f"   Growth: {coherence_stats['growth_mean']:.3f} ± {coherence_stats['growth_std']:.3f}")
    
    if council_stats:
        print(f"\n🏛️  Council Decisions:")
        print(f"   Total: {council_stats['total_decisions']}")
        print(f"   Approve Rate: {council_stats['outcome_rates'].get('Approve', 0)*100:.1f}%")
        print(f"   Mode Distribution: {council_stats['mode_distribution']}")
    
    if skill_stats:
        print(f"\n🎯 Skills:")
        print(f"   Promoted: {skill_stats['skills_promoted_mean']:.1f} ± {skill_stats['skills_promoted_std']:.1f}")
        print(f"   Jury Pass Rate: {skill_stats['jury_pass_rate_mean']*100:.1f}%")
    
    print(f"\n📁 Report saved: {output_dir / 'analysis_report.json'}")
    print("="*60)

if __name__ == '__main__':
    if len(sys.argv) < 2:
        print("Usage: python3 analyze_results.py <run_directory>")
        sys.exit(1)
    
    run_dir = Path(sys.argv[1])
    output_dir = run_dir / 'analysis'
    output_dir.mkdir(exist_ok=True)
    
    generate_report(run_dir, output_dir)
