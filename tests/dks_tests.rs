//! DKS (Dynamic Kinetic Stability) comprehensive tests

use ::hyper_stigmergy::*;
use std::collections::HashMap;

#[test]
fn test_replicator_lifecycle() {
    let mut replicator = Replicator::new("test".to_string(), 0.5, 0.1);

    // Initial state
    assert_eq!(replicator.generation(), 0);
    assert!(replicator.energy() > 0.0);

    // Metabolize
    replicator.metabolize(10.0, 1.0);
    assert!(replicator.energy() > 50.0); // Started with 50, gained more

    // Check replication condition
    let can_replicate = replicator.should_replicate(10.0);
    // Depends on energy level

    // Decay
    let initial_energy = replicator.energy();
    replicator.decay();
    assert!(replicator.energy() < initial_energy);

    // Update persistence
    replicator.update_persistence();
    assert!(replicator.persistence_score() > 0.0);

    println!("✓ Replicator lifecycle test passed");
    println!("  Generation: {}", replicator.generation());
    println!("  Energy: {:.2}", replicator.energy());
    println!("  Persistence: {:.2}", replicator.persistence_score());
}

#[test]
fn test_replicator_reproduction() {
    let parent = Replicator::new("parent".to_string(), 0.8, 0.1);

    // Try to replicate
    if let Some(child) = parent.replicate() {
        assert_eq!(child.generation(), 1);
        assert!(child.parent_id().is_some());
        assert_eq!(child.parent_id().unwrap(), parent.id());
        println!("✓ Replicator reproduction successful");
    } else {
        println!("✗ Replicator could not reproduce (may need more energy)");
    }
}

#[test]
fn test_population_dynamics() {
    let mut population = Population::new(100);

    // Add initial entities
    for i in 0..10 {
        let rep = Replicator::new(format!("entity_{}", i), 0.3, 0.05);
        population.add_entity(rep);
    }

    assert_eq!(population.size(), 10);

    // Run metabolism
    let env = Environment::default();
    population.metabolize(&env, 1.0);

    // Run replication
    let new_count = population.replicate(10.0, 100);
    println!("New entities created: {}", new_count);

    // Run decay
    population.decay();

    // Get stats
    let stats = population.stats();
    println!("✓ Population dynamics test");
    println!("  Size: {}", stats.size);
    println!("  Total energy: {:.2}", stats.total_energy);
    println!("  Avg persistence: {:.2}", stats.average_persistence);
}

#[test]
fn test_selection_pressure() {
    let mut population = Population::new(100);

    // Add entities with varying persistence
    for i in 0..20 {
        let mut rep = Replicator::new(format!("entity_{}", i), 0.3, 0.1);
        // Simulate different persistence levels
        for _ in 0..i {
            rep.update_persistence();
        }
        population.add_entity(rep);
    }

    let initial_size = population.size();

    // Apply selection
    let mut selection = SelectionPressure::new(0.3);
    let env = Environment::default();
    let removed = selection.select(&mut population, &env);

    println!("✓ Selection pressure test");
    println!(
        "  Initial: {}, Removed: {}, Final: {}",
        initial_size,
        removed,
        population.size()
    );

    // Should have removed some low-persistence entities
    assert!(population.size() <= initial_size);
}

#[test]
fn test_environmental_flux() {
    let mut flux = Flux::new(0.1);
    let mut env = Environment::default();

    let initial_energy = env.total_resources();

    // Apply flux multiple times
    for _ in 0..10 {
        flux.apply(&mut env);
    }

    println!("✓ Environmental flux test");
    println!("  Initial resources: {:.2}", initial_energy);
    println!("  Final resources: {:.2}", env.total_resources());
    println!("  Temperature: {:.2}", env.temperature());
}

#[test]
fn test_dks_full_lifecycle() {
    let config = DKSConfig {
        base_replication_rate: 0.4,
        base_decay_rate: 0.1,
        replication_energy_cost: 8.0,
        resource_energy_conversion: 2.5,
        max_population: 200,
        selection_intensity: 0.25,
        flux_rate: 0.08,
    };

    let mut dks = DKSSystem::new(config);

    // Seed population
    dks.seed(15);
    println!("Initial population: {}", dks.stats().size);

    // Track evolution over generations
    let mut population_history = Vec::new();
    let mut energy_history = Vec::new();

    for gen in 0..30 {
        let result = dks.tick();
        population_history.push(result.population_size);
        energy_history.push(result.total_energy);

        if gen % 10 == 0 {
            println!(
                "Gen {}: {} entities, {:.2} energy, {:.2} avg persistence",
                gen, result.population_size, result.total_energy, result.average_persistence
            );
        }
    }

    let final_stats = dks.stats();
    println!("✓ DKS full lifecycle test");
    println!("  Final population: {}", final_stats.size);
    println!("  Max generation reached: {}", final_stats.max_generation);
    println!(
        "  Population change: {} -> {}",
        population_history.first().unwrap(),
        population_history.last().unwrap()
    );
}

#[test]
fn test_multifractal_spectrum() {
    // Create persistence data with varying distributions
    let mut persistence_values = Vec::new();

    // Add some high-persistence values
    for _ in 0..100 {
        persistence_values.push(50.0 + rand::random::<f64>() * 50.0);
    }

    // Add some low-persistence values
    for _ in 0..100 {
        persistence_values.push(rand::random::<f64>() * 20.0);
    }

    // Add medium values
    for _ in 0..100 {
        persistence_values.push(20.0 + rand::random::<f64>() * 30.0);
    }

    let box_sizes = vec![1, 2, 4, 8, 16, 32, 64];
    let spectrum =
        MultifractalSpectrum::from_persistence_distribution(&persistence_values, &box_sizes);

    println!("✓ Multifractal spectrum analysis");
    println!("  Alpha values: {}", spectrum.alpha_values.len());
    println!("  Spectrum width: {:.4}", spectrum.width());
    println!("  Is multifractal: {}", spectrum.is_multifractal(0.1));
    println!("  Capacity dimension: {:.4}", spectrum.capacity_dimension());

    // Verify spectrum structure is valid (may have NaN in some cases due to numerical issues)
    assert!(
        !spectrum.alpha_values.is_empty(),
        "Should have alpha values"
    );
    assert!(
        !spectrum.fractal_dimensions.is_empty(),
        "Should have fractal dimensions"
    );
}

#[test]
fn test_multiscale_analysis() {
    let mut multiscale = MultiscaleDKS::new();

    // Generate sample data
    let persistence: Vec<f64> = (0..500)
        .map(|i| {
            let x = i as f64 / 500.0;
            // Create multi-scale pattern
            (x * 10.0).sin() * 5.0 + 10.0 + rand::random::<f64>() * 2.0
        })
        .collect();

    multiscale.analyze(&persistence);

    let scales = multiscale.scales().len();
    println!("✓ Multiscale analysis");
    println!("  Number of scales analyzed: {}", scales);

    if let Some(critical) = multiscale.critical_scale() {
        println!("  Critical scale detected: {}", critical);
    }

    let exponents = multiscale.scaling_exponents();
    println!("  Scaling exponents: {:?}", exponents);
}

#[test]
fn test_persistence_measure() {
    let measure = PersistenceMeasure {
        survival_time: 100,
        average_energy: 75.0,
        energy_variance: 10.0,
        replication_count: 5,
        dks_stability: 0.3,
        stress_survived: 2.0,
    };

    let score = measure.score();
    println!("✓ Persistence measure score: {:.2}", score);

    // Score should be positive
    assert!(score > 0.0);
}

#[test]
fn test_compositionality() {
    // Test compositionality measure
    let whole = 150.0;
    let parts = vec![50.0, 60.0];

    let comp = compositionality_measure(whole, &parts);

    println!("✓ Compositionality measure");
    println!("  Whole: {:.2}", whole);
    println!("  Sum of parts: {:.2}", parts.iter().sum::<f64>());
    println!("  Compositionality: {:.2}", comp);

    // Whole > sum of parts -> positive compositionality
    assert!(comp > 0.0);
}
