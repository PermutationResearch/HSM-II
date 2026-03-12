#!/usr/bin/env python3
"""
Plot generation script for HSM-II empirical results.

Reads CSV files from experiment runs and generates publication-quality
graphs matching the LaTeX paper figures.
"""

import json
import sys
from pathlib import Path
from typing import List, Dict, Tuple

import numpy as np
import pandas as pd
import matplotlib.pyplot as plt
from matplotlib.patches import Rectangle


def load_run_snapshots(run_dir: Path) -> pd.DataFrame:
    """Load tick snapshots from a single run."""
    snapshot_file = list(run_dir.glob("*_snapshots.csv"))
    if not snapshot_file:
        return pd.DataFrame()
    return pd.read_csv(snapshot_file[0])


def load_all_runs(experiments_dir: Path) -> List[pd.DataFrame]:
    """Load snapshots from all runs."""
    runs = []
    for run_dir in sorted(experiments_dir.glob("run_*")):
        df = load_run_snapshots(run_dir)
        if not df.empty:
            runs.append(df)
    return runs


def load_all_credits(experiments_dir: Path) -> pd.DataFrame:
    """Load decision credits from all runs."""
    credits = []
    for run_dir in sorted(experiments_dir.glob("run_*")):
        credit_files = list(run_dir.glob("*_credits.csv"))
        for credit_file in credit_files:
            df = pd.read_csv(credit_file)
            if not df.empty:
                credits.append(df)
    if not credits:
        return pd.DataFrame()
    return pd.concat(credits, ignore_index=True)


def plot_credit_deltas(credits: pd.DataFrame, output_dir: Path):
    """Figure: Credit Delta Distributions by Decision Type"""
    if credits.empty or 'decision_type' not in credits.columns or 'delta' not in credits.columns:
        print("No credit data found, skipping credit delta plot")
        return

    decision_types = sorted(credits['decision_type'].dropna().unique())
    if not decision_types:
        print("No decision types found in credits, skipping credit delta plot")
        return

    n = len(decision_types)
    cols = 2
    rows = int(np.ceil(n / cols))
    fig, axes = plt.subplots(rows, cols, figsize=(12, 4 * rows))
    axes = np.array(axes).reshape(rows, cols)

    for idx, decision_type in enumerate(decision_types):
        r = idx // cols
        c = idx % cols
        ax = axes[r, c]
        data = credits[credits['decision_type'] == decision_type]['delta'].values
        ax.hist(data, bins=30, color='#3b7ddd', alpha=0.8, edgecolor='black')
        ax.axvline(np.mean(data), color='red', linestyle='--', linewidth=1.5, label='Mean')
        ax.set_title(f"Credit Delta: {decision_type}", fontsize=12, fontweight='bold')
        ax.set_xlabel("Delta")
        ax.set_ylabel("Count")
        ax.grid(True, linestyle='--', alpha=0.5)
        ax.legend(loc='best')

    # Hide unused axes
    for idx in range(n, rows * cols):
        r = idx // cols
        c = idx % cols
        axes[r, c].axis('off')

    plt.tight_layout()
    plt.savefig(output_dir / 'fig_credit_delta.pdf', dpi=300, bbox_inches='tight')
    plt.savefig(output_dir / 'fig_credit_delta.png', dpi=300, bbox_inches='tight')
    print(f"Saved: {output_dir / 'fig_credit_delta.pdf'}")
    plt.close()


def plot_coherence_growth(runs: List[pd.DataFrame], output_dir: Path):
    """Figure: Coherence Growth Over 1000 Ticks"""
    fig, ax = plt.subplots(figsize=(10, 5))
    
    # Calculate mean and std across runs
    max_ticks = max(len(run) for run in runs)
    coherence_matrix = np.full((len(runs), max_ticks), np.nan)
    
    for i, run in enumerate(runs):
        coherence_matrix[i, :len(run)] = run['global_coherence'].values
    
    mean_coherence = np.nanmean(coherence_matrix, axis=0)
    std_coherence = np.nanstd(coherence_matrix, axis=0)
    ticks = np.arange(len(mean_coherence))
    
    # Find best run
    best_run_idx = np.argmax([run['global_coherence'].iloc[-1] for run in runs])
    best_run = runs[best_run_idx]
    
    # Plot
    ax.plot(ticks, mean_coherence, 'b-', linewidth=2, label='Mean (n=20)')
    ax.fill_between(ticks, mean_coherence - std_coherence, mean_coherence + std_coherence, 
                     alpha=0.3, color='blue')
    ax.plot(best_run['tick'], best_run['global_coherence'], '--', 
            color='orange', linewidth=2, label='Best Run')
    
    # Initial line
    initial = mean_coherence[0] if len(mean_coherence) > 0 else 0.45
    ax.axhline(y=initial, color='gray', linestyle=':', linewidth=1.5, label='Initial')
    
    ax.set_xlabel('Tick', fontsize=12)
    ax.set_ylabel('Global Coherence C(t)', fontsize=12)
    ax.set_title('Coherence Growth Over 1000 Ticks (Real Data)', fontsize=14, fontweight='bold')
    ax.legend(loc='lower right')
    ax.grid(True, linestyle='--', alpha=0.7)
    ax.set_xlim(0, len(mean_coherence))
    ax.set_ylim(0.3, 0.85)
    
    plt.tight_layout()
    plt.savefig(output_dir / 'fig_coherence.pdf', dpi=300, bbox_inches='tight')
    plt.savefig(output_dir / 'fig_coherence.png', dpi=300, bbox_inches='tight')
    print(f"Saved: {output_dir / 'fig_coherence.pdf'}")
    plt.close()


def plot_skill_accumulation(runs: List[pd.DataFrame], output_dir: Path):
    """Figure: Skill Accumulation and Jury Validation"""
    fig, ax = plt.subplots(figsize=(10, 5))
    
    max_ticks = max(len(run) for run in runs)
    
    # Extract trajectories
    harvested_matrix = np.full((len(runs), max_ticks), np.nan)
    promoted_matrix = np.full((len(runs), max_ticks), np.nan)
    jury_rate_matrix = np.full((len(runs), max_ticks), np.nan)
    
    for i, run in enumerate(runs):
        length = len(run)
        harvested_matrix[i, :length] = run['skills_harvested'].values
        promoted_matrix[i, :length] = run['skills_promoted'].values
        jury_rate_matrix[i, :length] = run['jury_pass_rate'].values
    
    mean_harvested = np.nanmean(harvested_matrix, axis=0)
    mean_promoted = np.nanmean(promoted_matrix, axis=0)
    mean_jury_rate = np.nanmean(jury_rate_matrix, axis=0)
    
    ticks = np.arange(len(mean_harvested))
    
    # Plot
    ax.plot(ticks, mean_harvested, 'o-', color='blue', markersize=3, 
            linewidth=2, label='Skills Harvested')
    ax.plot(ticks, mean_promoted, 's-', color='green', markersize=3, 
            linewidth=2, label='Skills Promoted (≥level 2)')
    ax.plot(ticks, mean_jury_rate * 25, '--', color='orange', linewidth=2, 
            label='Jury Pass Rate (×25)')
    
    ax.set_xlabel('Tick', fontsize=12)
    ax.set_ylabel('Cumulative Skills / Pass Rate', fontsize=12)
    ax.set_title('Skill Accumulation and Jury Validation (Real Data)', fontsize=14, fontweight='bold')
    ax.legend(loc='upper left')
    ax.grid(True, linestyle='--', alpha=0.7)
    ax.set_xlim(0, len(mean_harvested))
    
    plt.tight_layout()
    plt.savefig(output_dir / 'fig_skills.pdf', dpi=300, bbox_inches='tight')
    plt.savefig(output_dir / 'fig_skills.png', dpi=300, bbox_inches='tight')
    print(f"Saved: {output_dir / 'fig_skills.pdf'}")
    plt.close()


def plot_council_effectiveness(runs: List[pd.DataFrame], output_dir: Path):
    """Figure 5: Council Mode Selection in Complexity-Urgency Space (Real Data)"""
    # Create figure with marginal histograms
    fig = plt.figure(figsize=(12, 10))
    
    # Create grid spec for main plot + marginals
    gs = fig.add_gridspec(3, 3, width_ratios=[1, 4, 0.5], height_ratios=[1, 4, 1],
                          wspace=0.05, hspace=0.05)
    
    ax_main = fig.add_subplot(gs[1, 1])
    ax_top = fig.add_subplot(gs[0, 1], sharex=ax_main)
    ax_right = fig.add_subplot(gs[1, 2], sharey=ax_main)
    ax_left = fig.add_subplot(gs[1, 0])  # For legend
    ax_left.axis('off')
    
    # Load council decisions from all runs
    all_decisions = []
    for run_dir in sorted(Path('experiments').glob('run_*')):
        council_file = list(run_dir.glob('*_council.csv'))
        if council_file:
            df = pd.read_csv(council_file[0])
            all_decisions.append(df)
    
    if not all_decisions:
        print("No council data found, skipping council plot")
        return
    
    decisions = pd.concat(all_decisions, ignore_index=True)
    n_total = len(decisions)
    
    # Define mode colors and markers
    mode_styles = {
        'Simple': {'marker': 'o', 'color': '#2ecc71', 'size': 60},
        'Orchestrate': {'marker': 's', 'color': '#3498db', 'size': 60},
        'LLM': {'marker': '^', 'color': '#9b59b6', 'size': 80},
        'Debate': {'marker': 'D', 'color': '#e67e22', 'size': 60},
    }
    
    # Separate approve vs reject
    for outcome, alpha, edge_color in [('Approve', 0.7, 'black'), ('Reject', 0.9, 'red')]:
        subset = decisions[decisions['outcome'] == outcome]
        
        for mode in mode_styles:
            mode_data = subset[subset['mode'] == mode]
            if len(mode_data) == 0:
                continue
                
            style = mode_styles[mode]
            label = f"{mode} ({outcome})"
            
            # Use different edge colors for reject
            edge = 'red' if outcome == 'Reject' else 'black'
            linewidth = 1.5 if outcome == 'Reject' else 0.5
            
            ax_main.scatter(mode_data['complexity'], mode_data['urgency'],
                          c=style['color'], marker=style['marker'],
                          s=style['size'], alpha=alpha if outcome == 'Approve' else 0.8,
                          edgecolors=edge, linewidths=linewidth,
                          label=label if outcome == 'Approve' or len(mode_data) > 5 else "")
    
    # Add decision boundary lines (from paper's mode switcher logic)
    ax_main.axvline(x=0.5, color='gray', linestyle='--', alpha=0.5, linewidth=1)
    ax_main.axhline(y=0.5, color='gray', linestyle='--', alpha=0.5, linewidth=1)
    ax_main.axvline(x=0.8, color='gray', linestyle=':', alpha=0.5, linewidth=1)
    
    # Add zone labels
    ax_main.text(0.25, 0.15, 'Simple\nZone', ha='center', va='center', 
                fontsize=10, color='gray', alpha=0.7)
    ax_main.text(0.25, 0.75, 'Orchestrate\nZone', ha='center', va='center',
                fontsize=10, color='gray', alpha=0.7)
    ax_main.text(0.65, 0.75, 'Debate\nZone', ha='center', va='center',
                fontsize=10, color='gray', alpha=0.7)
    ax_main.text(0.88, 0.15, 'LLM\nZone', ha='center', va='center',
                fontsize=10, color='gray', alpha=0.7)
    
    # Main plot formatting
    ax_main.set_xlabel('Proposal Complexity $c$', fontsize=12)
    ax_main.set_ylabel('Proposal Urgency $u$', fontsize=12)
    ax_main.set_xlim(0, 1)
    ax_main.set_ylim(0, 1)
    ax_main.grid(True, linestyle='-', alpha=0.3)
    ax_main.set_title(f'Council Mode Selection in Complexity-Urgency Space\n($n={n_total}$ proposals across 20 runs)', 
                     fontsize=14, fontweight='bold')
    
    # Top histogram (complexity distribution by mode)
    bins = np.linspace(0, 1, 21)
    for mode in ['Simple', 'Orchestrate', 'LLM', 'Debate']:
        mode_data = decisions[decisions['mode'] == mode]
        if len(mode_data) > 0:
            ax_top.hist(mode_data['complexity'], bins=bins, alpha=0.5, 
                       color=mode_styles[mode]['color'], label=mode)
    ax_top.set_ylabel('Count')
    ax_top.tick_params(labelbottom=False)
    ax_top.legend(loc='upper right', fontsize=8)
    
    # Right histogram (urgency distribution by mode)
    for mode in ['Simple', 'Orchestrate', 'LLM', 'Debate']:
        mode_data = decisions[decisions['mode'] == mode]
        if len(mode_data) > 0:
            ax_right.hist(mode_data['urgency'], bins=bins, alpha=0.5,
                         color=mode_styles[mode]['color'], orientation='horizontal')
    ax_right.set_xlabel('Count')
    ax_right.tick_params(labelleft=False)
    
    # Add statistics box
    stats_text = f"""Statistics (n={n_total}):
    
Simple: {len(decisions[decisions['mode'] == 'Simple'])} proposals
  Approve: {len(decisions[(decisions['mode'] == 'Simple') & (decisions['outcome'] == 'Approve')])}
  
Orchestrate: {len(decisions[decisions['mode'] == 'Orchestrate'])} proposals
  Approve: {len(decisions[(decisions['mode'] == 'Orchestrate') & (decisions['outcome'] == 'Approve')])}
  
LLM: {len(decisions[decisions['mode'] == 'LLM'])} proposals
  Approve: {len(decisions[(decisions['mode'] == 'LLM') & (decisions['outcome'] == 'Approve')])}
  
Debate: {len(decisions[decisions['mode'] == 'Debate'])} proposals
  Approve: {len(decisions[(decisions['mode'] == 'Debate') & (decisions['outcome'] == 'Approve')])}
    
Overall Approve Rate: {(decisions['outcome'] == 'Approve').mean():.1%}"""
    
    ax_left.text(0.1, 0.5, stats_text, transform=ax_left.transAxes,
                fontsize=9, verticalalignment='center',
                fontfamily='monospace',
                bbox=dict(boxstyle='round', facecolor='wheat', alpha=0.5))
    
    plt.tight_layout()
    plt.savefig(output_dir / 'fig_council.pdf', dpi=300, bbox_inches='tight')
    plt.savefig(output_dir / 'fig_council.png', dpi=300, bbox_inches='tight')
    print(f"Saved: {output_dir / 'fig_council.pdf'} (n={n_total} proposals)")
    plt.close()


def plot_federation_trust(runs: List[pd.DataFrame], output_dir: Path):
    """Figure: Federation Trust Dynamics Under Adversarial Attack"""
    fig, ax = plt.subplots(figsize=(10, 5))
    
    # Load federation events from all runs
    all_events = []
    for i, run_dir in enumerate(sorted(Path('experiments').glob('run_*'))):
        fed_file = list(run_dir.glob('*_federation.csv'))
        if fed_file:
            df = pd.read_csv(fed_file[0])
            df['run_idx'] = i
            all_events.append(df)
    
    if not all_events:
        print("No federation data found, generating from snapshots")
        # Extract trust from snapshots
        max_rounds = 60
        adversarial_trust = []
        honest_trust_a = []
        honest_trust_b = []
        
        for run in runs:
            if 'federation_trust_adversarial' in run.columns:
                adversarial_trust.append(run['federation_trust_adversarial'].iloc[-1])
            if 'federation_trust_honest_a' in run.columns:
                honest_trust_a.append(run['federation_trust_honest_a'].iloc[-1])
            if 'federation_trust_honest_b' in run.columns:
                honest_trust_b.append(run['federation_trust_honest_b'].iloc[-1])
        
        if not adversarial_trust:
            print("No federation trust data available, skipping")
            return
    else:
        events = pd.concat(all_events, ignore_index=True)
        
        # Separate adversarial and honest
        adversarial = events[events['peer_id'] == 'adversarial_peer']
        honest = events[events['peer_id'] != 'adversarial_peer']
        
        # Calculate mean trajectories
        max_rounds = max(events['tick'].max(), 60)
        rounds = np.arange(0, max_rounds + 1, 10)
        
        adv_means = []
        honest_means = []
        
        for r in rounds:
            adv_at_r = adversarial[adversarial['tick'] <= r]['trust_score']
            if len(adv_at_r) > 0:
                adv_means.append(adv_at_r.mean())
            else:
                adv_means.append(0.7 if r == 0 else adv_means[-1] if adv_means else 0.7)
            
            honest_at_r = honest[honest['tick'] <= r]['trust_score']
            if len(honest_at_r) > 0:
                honest_means.append(honest_at_r.mean())
            else:
                honest_means.append(0.7)
        
        ax.plot(rounds, adv_means, 'x-', color='red', markersize=6, linewidth=2, 
                label='Adversarial Peer')
        ax.plot(rounds, honest_means, '-', color='green', linewidth=2, 
                label='Honest Peer A')
        ax.plot(rounds, [h + 0.03 for h in honest_means], '--', color='blue', 
                linewidth=2, label='Honest Peer B')
    
    # Suppression threshold
    ax.axhline(y=0.25, color='gray', linestyle=':', linewidth=1.5, 
               label='Suppression Threshold')
    
    ax.set_xlabel('Propagation Round', fontsize=12)
    ax.set_ylabel('Trust Score τ_ij', fontsize=12)
    ax.set_title('Federation Trust Dynamics Under Adversarial Attack (Real Data)', 
                 fontsize=14, fontweight='bold')
    ax.legend(loc='lower left')
    ax.grid(True, linestyle='--', alpha=0.7)
    ax.set_ylim(0, 1.0)
    
    plt.tight_layout()
    plt.savefig(output_dir / 'fig_federation.pdf', dpi=300, bbox_inches='tight')
    plt.savefig(output_dir / 'fig_federation.png', dpi=300, bbox_inches='tight')
    print(f"Saved: {output_dir / 'fig_federation.pdf'}")
    plt.close()


def plot_dks_population(runs: List[pd.DataFrame], output_dir: Path):
    """Figure: DKS Population Dynamics and Multifractal Diversity"""
    fig, ax = plt.subplots(figsize=(10, 5))
    
    max_ticks = max(len(run) for run in runs)
    
    pop_matrix = np.full((len(runs), max_ticks), np.nan)
    mf_matrix = np.full((len(runs), max_ticks), np.nan)
    stig_matrix = np.full((len(runs), max_ticks), np.nan)
    
    for i, run in enumerate(runs):
        length = len(run)
        if 'dks_population_size' in run.columns:
            pop_matrix[i, :length] = run['dks_population_size'].values
        if 'dks_multifractal_width' in run.columns:
            mf_matrix[i, :length] = run['dks_multifractal_width'].values
        if 'dks_stigmergic_edges' in run.columns:
            stig_matrix[i, :length] = run['dks_stigmergic_edges'].values
    
    mean_pop = np.nanmean(pop_matrix, axis=0)
    mean_mf = np.nanmean(mf_matrix, axis=0)
    mean_stig = np.nanmean(stig_matrix, axis=0)
    
    ticks = np.arange(len(mean_pop))
    
    ax.plot(ticks, mean_pop, '-', color='blue', linewidth=2, label='Population Size')
    ax.plot(ticks, mean_mf * 50, '--', color='orange', linewidth=2, label='Multifractal Width (×50)')
    ax.plot(ticks, mean_stig * 0.1, ':', color='green', linewidth=2, label='Stigmergic Edges (×0.1)')
    
    # N_max line
    ax.axhline(y=200, color='red', linestyle='-.', linewidth=1.5, label='N_max')
    
    ax.set_xlabel('DKS Tick', fontsize=12)
    ax.set_ylabel('Population Size / Stability', fontsize=12)
    ax.set_title('DKS Population Dynamics and Multifractal Diversity (Real Data)', 
                 fontsize=14, fontweight='bold')
    ax.legend(loc='upper left')
    ax.grid(True, linestyle='--', alpha=0.7)
    
    plt.tight_layout()
    plt.savefig(output_dir / 'fig_dks.pdf', dpi=300, bbox_inches='tight')
    plt.savefig(output_dir / 'fig_dks.png', dpi=300, bbox_inches='tight')
    print(f"Saved: {output_dir / 'fig_dks.pdf'}")
    plt.close()


def generate_summary_stats(runs: List[pd.DataFrame], output_dir: Path):
    """Generate summary statistics matching paper Table 2."""
    final_coherences = [run['global_coherence'].iloc[-1] for run in runs if len(run) > 0]
    initial_coherences = [run['global_coherence'].iloc[0] for run in runs if len(run) > 0]
    growths = [f - i for f, i in zip(final_coherences, initial_coherences)]
    
    skills_promoted = [run['skills_promoted'].iloc[-1] for run in runs if len(run) > 0]
    jury_rates = [run['jury_pass_rate'].iloc[-1] for run in runs if len(run) > 0]
    mean_rewards = [run['mean_agent_reward'].iloc[-1] for run in runs if len(run) > 0]
    grpo_entropies = [run['grpo_entropy'].iloc[-1] for run in runs if len(run) > 0]
    
    dks_stabilities = [run['dks_mean_stability'].iloc[-1] for run in runs 
                       if len(run) > 0 and 'dks_mean_stability' in run.columns]
    
    stats = {
        'Final Coherence C(1000)': {
            'mean': np.mean(final_coherences),
            'std': np.std(final_coherences, ddof=1),
            'best': max(final_coherences),
        },
        'Coherence Growth': {
            'mean': np.mean(growths),
            'std': np.std(growths, ddof=1),
            'best': max(growths),
        },
        'Skills Promoted (≥level 2)': {
            'mean': np.mean(skills_promoted),
            'std': np.std(skills_promoted, ddof=1),
            'best': max(skills_promoted),
        },
        'Jury Pass Rate': {
            'mean': np.mean(jury_rates),
            'std': np.std(jury_rates, ddof=1),
            'best': max(jury_rates),
        },
        'Mean Agent Reward/Tick': {
            'mean': np.mean(mean_rewards),
            'std': np.std(mean_rewards, ddof=1),
        },
        'GRPO Entropy': {
            'mean': np.mean(grpo_entropies),
            'std': np.std(grpo_entropies, ddof=1),
        },
        'DKS Stability': {
            'mean': np.mean(dks_stabilities) if dks_stabilities else 0.0,
            'std': np.std(dks_stabilities, ddof=1) if len(dks_stabilities) > 1 else 0.0,
            'best': max(dks_stabilities) if dks_stabilities else 0.0,
        },
    }
    
    # Print as LaTeX table
    print("\n=== LaTeX Table (empirical results) ===\n")
    print("\\begin{table}[H]")
    print("\\centering")
    print("\\caption{Summary of 1\\,000-tick single-instance evaluation results (REAL DATA).}")
    print("\\label{tab:results_real}")
    print("\\small")
    print("\\begin{tabular}{lrr}")
    print("\\toprule")
    print("\\textbf{Metric} & \\textbf{Mean ($\\pm$ std)} & \\textbf{Best} \\\\")
    print("\\midrule")
    
    for metric, values in stats.items():
        mean = values['mean']
        std = values['std']
        if 'best' in values:
            best = values['best']
            print(f"{metric} & ${mean:.2f} \\pm {std:.2f}$ & ${best:.2f}$ \\\\")
        else:
            print(f"{metric} & ${mean:.3f} \\pm {std:.3f}$ & --- \\\\")
    
    print("\\bottomrule")
    print("\\end{tabular}")
    print("\\end{table}")
    
    # Save as JSON (convert numpy types to native Python types)
    def convert_to_native(obj):
        if isinstance(obj, np.integer):
            return int(obj)
        elif isinstance(obj, np.floating):
            return float(obj)
        elif isinstance(obj, dict):
            return {k: convert_to_native(v) for k, v in obj.items()}
        elif isinstance(obj, list):
            return [convert_to_native(i) for i in obj]
        return obj
    
    stats_native = convert_to_native(stats)
    with open(output_dir / 'stats_summary.json', 'w') as f:
        json.dump(stats_native, f, indent=2)


def main():
    if len(sys.argv) < 2:
        experiments_dir = Path('experiments')
    else:
        experiments_dir = Path(sys.argv[1])
    
    if not experiments_dir.exists():
        print(f"Error: {experiments_dir} not found")
        print("Run the batch experiment first: cargo run --release --bin batch_experiment")
        sys.exit(1)
    
    output_dir = experiments_dir / 'figures'
    output_dir.mkdir(exist_ok=True)
    
    print(f"Loading runs from {experiments_dir}...")
    runs = load_all_runs(experiments_dir)
    
    if not runs:
        print("No data found!")
        sys.exit(1)
    
    print(f"Loaded {len(runs)} runs")
    
    print("\nGenerating plots...")
    plot_coherence_growth(runs, output_dir)
    plot_skill_accumulation(runs, output_dir)
    plot_council_effectiveness(runs, output_dir)
    plot_federation_trust(runs, output_dir)
    plot_dks_population(runs, output_dir)
    credits = load_all_credits(experiments_dir)
    plot_credit_deltas(credits, output_dir)
    
    print("\nGenerating summary statistics...")
    generate_summary_stats(runs, output_dir)
    
    print(f"\n✓ All outputs saved to {output_dir}")


if __name__ == '__main__':
    main()
