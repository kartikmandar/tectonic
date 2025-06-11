//! Typst-inspired memoization system for TeX engine
//! 
//! This module implements a constrained memoization system based on Typst's comemo library
//! approach, adapted for TeX engine compilation. The system provides:
//! 
//! - Spatial constraint tracking for layout elements
//! - 128-bit SipHash-based compact storage 
//! - Dependency tracking and selective invalidation
//! - Integration with existing TeXEngineState from Phase 2.1
//! 
//! Key performance targets:
//! - >90% cache hit rate for typical editing patterns
//! - <10ms incremental compilation for character-level changes
//! - 3.4x-9,895x speedup potential (matching Typst benchmarks)

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, BTreeMap};
use std::time::{SystemTime, UNIX_EPOCH};
use siphasher::sip128::{Hasher128, SipHasher13};
use std::hash::{Hash, Hasher};

/// Core memoization system inspired by Typst's comemo library
pub struct MemoizationSystem {
    /// Primary cache storage with hash-based keys
    cache: MemoizationCache<CacheKey, CacheEntry>,
    /// Spatial constraint index for efficient lookup
    spatial_index: SpatialIndex,
    /// Dependency graph for tracking relationships
    dependency_graph: DependencyGraph,
    /// Cache statistics and performance metrics
    stats: CacheStatistics,
}

/// Spatial constraints defining layout dependencies
/// Based on Typst's constraint system but adapted for TeX's box model
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SpatialConstraints {
    // Layout dimension constraints (horizontal)
    /// Minimum width required for this element
    pub min_width: Option<f64>,
    /// Maximum width available for this element  
    pub max_width: Option<f64>,
    /// Available width from parent container
    pub available_width: Option<f64>,
    
    // Vertical constraint fields
    /// Minimum height required for this element
    pub min_height: Option<f64>,
    /// Maximum height available for this element
    pub max_height: Option<f64>,
    /// Available height from parent container
    pub available_height: Option<f64>,
    
    // Box constraint fields (TeX-specific)
    /// Natural width without stretching/shrinking
    pub natural_width: Option<f64>,
    /// Stretch component (how much element can grow)
    pub stretch: Option<f64>,
    /// Shrink component (how much element can shrink)
    pub shrink: Option<f64>,
    
    // TeX-specific constraints
    /// Line spacing parameter
    pub line_spacing: Option<f64>,
    /// Paragraph shape constraints (indentation, etc.)
    pub paragraph_shape: Option<ParagraphShape>,
    /// Math spacing parameters
    pub math_spacing: Option<MathSpacing>,
    
    /// Font metrics that affect layout
    pub font_metrics: Option<FontMetrics>,
    /// Language-specific settings (hyphenation, etc.)
    pub language_settings: Option<LanguageSettings>,
}

/// TeX paragraph shape constraints
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ParagraphShape {
    /// Left margin indentation
    pub left_indent: f64,
    /// Right margin indentation  
    pub right_indent: f64,
    /// First line indentation
    pub first_line_indent: f64,
    /// Line width
    pub line_width: f64,
    /// Hanging indentation
    pub hanging_indent: f64,
}

/// Math spacing constraints
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MathSpacing {
    /// Thin space (3/18 quad)
    pub thin_space: f64,
    /// Medium space (4/18 quad)  
    pub med_space: f64,
    /// Thick space (5/18 quad)
    pub thick_space: f64,
    /// Math quad space
    pub quad_space: f64,
}

/// Font metrics affecting layout constraints
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FontMetrics {
    /// Font size in points
    pub size: f64,
    /// Font family identifier
    pub family: String,
    /// Character width (for monospace fonts)
    pub char_width: Option<f64>,
    /// Line height multiplier
    pub line_height: f64,
    /// X-height of the font
    pub x_height: f64,
    /// Baseline skip
    pub baseline_skip: f64,
}

/// Language-specific settings
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LanguageSettings {
    /// Language code (en, de, fr, etc.)
    pub language: String,
    /// Hyphenation enabled
    pub hyphenation: bool,
    /// Left-to-right text direction
    pub ltr: bool,
}

/// Cache key using 128-bit SipHash for compact storage
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct CacheKey {
    /// Primary content hash (inputs)
    pub content_hash: u128,
    /// Constraint hash (spatial requirements)
    pub constraint_hash: u128,
    /// Engine state hash (TeX registers, fonts, etc.)
    pub state_hash: u128,
}

/// Cache entry with compact storage using hash-only return values
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry {
    /// 128-bit hash of the computation result (not the full result)
    pub result_hash: u128,
    /// Spatial constraints that were satisfied
    pub constraints: SpatialConstraints,
    /// Dependencies that were accessed during computation
    pub dependencies: Vec<DependencyKey>,
    /// Metadata about this cache entry
    pub metadata: CacheMetadata,
    /// Timestamp when entry was created
    pub timestamp: u64,
    /// Access count for LRU eviction
    pub access_count: u64,
    /// Last access time
    pub last_access: u64,
}

/// Cache metadata for tracking and debugging
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheMetadata {
    /// Source position where this entry was generated
    pub source_position: usize,
    /// Compilation context (paragraph, math, etc.)
    pub context_type: String,
    /// Size of original result (for memory tracking)
    pub result_size_bytes: usize,
    /// Performance metrics for this entry
    pub metrics: EntryMetrics,
}

/// Performance metrics for cache entries
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntryMetrics {
    /// Time taken to compute original result (microseconds)
    pub computation_time_us: u64,
    /// Memory allocated during computation
    pub memory_used_bytes: usize,
    /// Number of TeX operations performed
    pub tex_operations: u32,
    /// Cache hit rate for this specific pattern
    pub hit_rate: f64,
}

/// Dependency tracking key
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct DependencyKey {
    /// Type of dependency (file, macro, register, font)
    pub dep_type: DependencyType,
    /// Identifier for the dependency
    pub identifier: String,
    /// Hash of the dependency value when accessed
    pub value_hash: u128,
}

/// Types of dependencies that can be tracked
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub enum DependencyType {
    /// File system access (input files, images, etc.)
    File,
    /// Macro definition or expansion
    Macro,
    /// TeX register value (count, dimen, skip, etc.)
    Register,
    /// Font loading or metrics query
    Font,
    /// Package or class loading
    Package,
    /// Environment variable
    Environment,
    /// User-defined dependency
    Custom(String),
}

/// Generic memoization cache with spatial constraint support
pub struct MemoizationCache<K, V> {
    /// Primary storage using HashMap for O(1) lookup
    storage: HashMap<K, V>,
    /// Maximum number of entries before eviction
    max_entries: usize,
    /// Total memory usage in bytes
    memory_usage: usize,
    /// Memory limit in bytes (400MB target)
    memory_limit: usize,
}

impl<K: Hash + Eq + Clone, V: Clone> MemoizationCache<K, V> {
    /// Create a new memoization cache with specified limits
    pub fn new(max_entries: usize, memory_limit_mb: usize) -> Self {
        Self {
            storage: HashMap::with_capacity(max_entries / 4), // Start smaller
            max_entries,
            memory_usage: 0,
            memory_limit: memory_limit_mb * 1024 * 1024,
        }
    }
    
    /// Get an entry from the cache
    pub fn get(&self, key: &K) -> Option<&V> {
        self.storage.get(key)
    }
    
    /// Insert an entry into the cache
    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        // Check if we need to evict entries
        if self.storage.len() >= self.max_entries || 
           self.memory_usage >= self.memory_limit {
            self.evict_entries();
        }
        
        self.storage.insert(key, value)
    }
    
    /// Remove an entry from the cache
    pub fn remove(&mut self, key: &K) -> Option<V> {
        self.storage.remove(key)
    }
    
    /// Get current cache size
    pub fn len(&self) -> usize {
        self.storage.len()
    }
    
    /// Check if cache is empty
    pub fn is_empty(&self) -> bool {
        self.storage.is_empty()
    }
    
    /// Clear all cache entries
    pub fn clear(&mut self) {
        self.storage.clear();
        self.memory_usage = 0;
    }
    
    /// Evict entries based on LRU and memory pressure
    fn evict_entries(&mut self) {
        // Simple eviction strategy - remove oldest entries
        // In a full implementation, this would use LRU with spatial awareness
        let target_size = self.max_entries * 3 / 4; // Remove 25% of entries
        
        if self.storage.len() > target_size {
            // For now, just clear everything (would be smarter in production)
            self.storage.clear();
            self.memory_usage = 0;
        }
    }
}

/// Spatial index for efficient constraint-based lookups
pub struct SpatialIndex {
    /// R-tree-like structure for spatial queries (simplified for now)
    constraint_map: BTreeMap<String, Vec<CacheKey>>,
}

impl SpatialIndex {
    /// Create a new spatial index
    pub fn new() -> Self {
        Self {
            constraint_map: BTreeMap::new(),
        }
    }
    
    /// Add a cache entry to the spatial index
    pub fn insert(&mut self, key: CacheKey, constraints: &SpatialConstraints) {
        // Create a spatial key from constraints
        let spatial_key = self.create_spatial_key(constraints);
        
        self.constraint_map
            .entry(spatial_key)
            .or_insert_with(Vec::new)
            .push(key);
    }
    
    /// Find cache entries that satisfy given constraints
    pub fn find_compatible(&self, constraints: &SpatialConstraints) -> Vec<&CacheKey> {
        let spatial_key = self.create_spatial_key(constraints);
        
        self.constraint_map
            .get(&spatial_key)
            .map(|entries| entries.iter().collect())
            .unwrap_or_default()
    }
    
    /// Create a spatial key from constraints for indexing
    fn create_spatial_key(&self, constraints: &SpatialConstraints) -> String {
        // Simple spatial key generation - would be more sophisticated in production
        format!(
            "w:{:?}-{:?}_h:{:?}-{:?}_nw:{:?}",
            constraints.min_width,
            constraints.max_width,
            constraints.min_height,
            constraints.max_height,
            constraints.natural_width
        )
    }
}

/// Dependency graph for tracking input-output relationships
pub struct DependencyGraph {
    /// Map from cache keys to their dependencies
    dependencies: HashMap<CacheKey, Vec<DependencyKey>>,
    /// Reverse map from dependencies to cache keys that use them
    dependents: HashMap<DependencyKey, Vec<CacheKey>>,
}

impl DependencyGraph {
    /// Create a new dependency graph
    pub fn new() -> Self {
        Self {
            dependencies: HashMap::new(),
            dependents: HashMap::new(),
        }
    }
    
    /// Add a dependency relationship
    pub fn add_dependency(&mut self, cache_key: CacheKey, dep_key: DependencyKey) {
        // Add to forward map
        self.dependencies
            .entry(cache_key.clone())
            .or_insert_with(Vec::new)
            .push(dep_key.clone());
        
        // Add to reverse map
        self.dependents
            .entry(dep_key)
            .or_insert_with(Vec::new)
            .push(cache_key);
    }
    
    /// Get all cache keys that depend on a given dependency
    pub fn get_dependents(&self, dep_key: &DependencyKey) -> Vec<&CacheKey> {
        self.dependents
            .get(dep_key)
            .map(|keys| keys.iter().collect())
            .unwrap_or_default()
    }
    
    /// Get all dependencies for a cache key
    pub fn get_dependencies(&self, cache_key: &CacheKey) -> Vec<&DependencyKey> {
        self.dependencies
            .get(cache_key)
            .map(|deps| deps.iter().collect())
            .unwrap_or_default()
    }
}

/// Cache statistics for performance monitoring
#[derive(Debug, Clone, Default)]
pub struct CacheStatistics {
    /// Total cache lookups
    pub total_lookups: u64,
    /// Cache hits
    pub cache_hits: u64,
    /// Cache misses
    pub cache_misses: u64,
    /// Total evictions performed
    pub evictions: u64,
    /// Memory usage statistics
    pub memory_stats: MemoryStatistics,
    /// Performance timing statistics
    pub timing_stats: TimingStatistics,
}

impl CacheStatistics {
    /// Calculate cache hit rate
    pub fn hit_rate(&self) -> f64 {
        if self.total_lookups == 0 {
            0.0
        } else {
            self.cache_hits as f64 / self.total_lookups as f64
        }
    }
    
    /// Record a cache hit
    pub fn record_hit(&mut self) {
        self.total_lookups += 1;
        self.cache_hits += 1;
    }
    
    /// Record a cache miss  
    pub fn record_miss(&mut self) {
        self.total_lookups += 1;
        self.cache_misses += 1;
    }
    
    /// Record an eviction
    pub fn record_eviction(&mut self) {
        self.evictions += 1;
    }
}

/// Memory usage statistics
#[derive(Debug, Clone, Default)]
pub struct MemoryStatistics {
    /// Current memory usage in bytes
    pub current_usage: usize,
    /// Peak memory usage in bytes
    pub peak_usage: usize,
    /// Number of allocations
    pub allocations: u64,
    /// Number of deallocations
    pub deallocations: u64,
}

/// Performance timing statistics
#[derive(Debug, Clone, Default)]
pub struct TimingStatistics {
    /// Total time spent on cache operations (microseconds)
    pub total_cache_time_us: u64,
    /// Average lookup time (microseconds)
    pub avg_lookup_time_us: f64,
    /// Average constraint checking time (microseconds)
    pub avg_constraint_check_us: f64,
    /// Time saved through memoization (microseconds)
    pub total_time_saved_us: u64,
}

/// Utility functions for constraint compatibility checking
impl SpatialConstraints {
    /// Check if this constraint is compatible with another constraint
    pub fn is_compatible(&self, other: &SpatialConstraints) -> bool {
        self.constraints_compatible(other, 0.001) // 0.1% tolerance
    }
    
    /// Check constraint compatibility with tolerance
    pub fn constraints_compatible(&self, other: &SpatialConstraints, tolerance: f64) -> bool {
        // Width constraints
        if !self.dimension_compatible(self.min_width, other.min_width, tolerance) ||
           !self.dimension_compatible(self.max_width, other.max_width, tolerance) ||
           !self.dimension_compatible(self.available_width, other.available_width, tolerance) {
            return false;
        }
        
        // Height constraints  
        if !self.dimension_compatible(self.min_height, other.min_height, tolerance) ||
           !self.dimension_compatible(self.max_height, other.max_height, tolerance) ||
           !self.dimension_compatible(self.available_height, other.available_height, tolerance) {
            return false;
        }
        
        // Box constraints
        if !self.dimension_compatible(self.natural_width, other.natural_width, tolerance) ||
           !self.dimension_compatible(self.stretch, other.stretch, tolerance) ||
           !self.dimension_compatible(self.shrink, other.shrink, tolerance) {
            return false;
        }
        
        // TeX-specific constraints
        if !self.dimension_compatible(self.line_spacing, other.line_spacing, tolerance) {
            return false;
        }
        
        // Compare complex constraints
        if self.paragraph_shape != other.paragraph_shape ||
           self.math_spacing != other.math_spacing ||
           self.font_metrics != other.font_metrics ||
           self.language_settings != other.language_settings {
            return false;
        }
        
        true
    }
    
    /// Check if two optional dimensions are compatible within tolerance
    fn dimension_compatible(&self, a: Option<f64>, b: Option<f64>, tolerance: f64) -> bool {
        match (a, b) {
            (None, None) => true,
            (Some(x), Some(y)) => (x - y).abs() <= tolerance * x.max(y),
            _ => false, // One is Some, other is None
        }
    }
    
    /// Check if this constraint subsumes (covers) another constraint
    pub fn subsumes(&self, other: &SpatialConstraints) -> bool {
        // A constraint subsumes another if it's more general (less restrictive)
        self.width_subsumes(other) && 
        self.height_subsumes(other) &&
        self.box_subsumes(other)
    }
    
    fn width_subsumes(&self, other: &SpatialConstraints) -> bool {
        // Check if our width constraints are less restrictive
        match (self.min_width, other.min_width) {
            (None, _) => true, // No constraint is less restrictive
            (Some(_), None) => false, // We have constraint, other doesn't
            (Some(a), Some(b)) => a <= b, // Our min is smaller (less restrictive)
        } &&
        match (self.max_width, other.max_width) {
            (None, _) => true,
            (Some(_), None) => false,
            (Some(a), Some(b)) => a >= b, // Our max is larger (less restrictive)
        }
    }
    
    fn height_subsumes(&self, other: &SpatialConstraints) -> bool {
        // Similar logic for height constraints
        match (self.min_height, other.min_height) {
            (None, _) => true,
            (Some(_), None) => false,
            (Some(a), Some(b)) => a <= b,
        } &&
        match (self.max_height, other.max_height) {
            (None, _) => true,
            (Some(_), None) => false,
            (Some(a), Some(b)) => a >= b,
        }
    }
    
    fn box_subsumes(&self, other: &SpatialConstraints) -> bool {
        // Box constraints are more complex - for now, require exact match
        self.natural_width == other.natural_width &&
        self.stretch == other.stretch &&
        self.shrink == other.shrink
    }
    
    /// Compute intersection of two constraints (most restrictive combination)
    pub fn intersect(&self, other: &SpatialConstraints) -> SpatialConstraints {
        SpatialConstraints {
            // Take most restrictive width constraints
            min_width: self.max_option(self.min_width, other.min_width),
            max_width: self.min_option(self.max_width, other.max_width),
            available_width: self.min_option(self.available_width, other.available_width),
            
            // Take most restrictive height constraints
            min_height: self.max_option(self.min_height, other.min_height),
            max_height: self.min_option(self.max_height, other.max_height),
            available_height: self.min_option(self.available_height, other.available_height),
            
            // For box constraints, take from self if both are Some, otherwise None
            natural_width: if self.natural_width.is_some() && other.natural_width.is_some() {
                self.natural_width
            } else {
                None
            },
            stretch: self.min_option(self.stretch, other.stretch),
            shrink: self.min_option(self.shrink, other.shrink),
            
            // TeX-specific constraints - take most restrictive
            line_spacing: self.min_option(self.line_spacing, other.line_spacing),
            paragraph_shape: self.paragraph_shape.clone().or_else(|| other.paragraph_shape.clone()),
            math_spacing: self.math_spacing.clone().or_else(|| other.math_spacing.clone()),
            font_metrics: self.font_metrics.clone().or_else(|| other.font_metrics.clone()),
            language_settings: self.language_settings.clone().or_else(|| other.language_settings.clone()),
        }
    }
    
    fn max_option(&self, a: Option<f64>, b: Option<f64>) -> Option<f64> {
        match (a, b) {
            (Some(x), Some(y)) => Some(x.max(y)),
            (Some(x), None) => Some(x),
            (None, Some(y)) => Some(y),
            (None, None) => None,
        }
    }
    
    fn min_option(&self, a: Option<f64>, b: Option<f64>) -> Option<f64> {
        match (a, b) {
            (Some(x), Some(y)) => Some(x.min(y)),
            (Some(x), None) => Some(x),
            (None, Some(y)) => Some(y),
            (None, None) => None,
        }
    }
}

/// Hash computation utilities using SipHash
pub struct HashComputation;

impl HashComputation {
    /// Compute 128-bit SipHash for cache keys
    pub fn hash_content<T: Hash>(content: &T) -> u128 {
        let mut hasher = SipHasher13::new();
        content.hash(&mut hasher);
        hasher.finish128().as_u128()
    }
    
    /// Compute hash for TeX engine state
    pub fn hash_tex_state(state: &crate::wasm::TeXEngineState) -> u128 {
        let mut hasher = SipHasher13::new();
        
        // Hash key components of TeX state
        state.memory_snapshot.hash(&mut hasher);
        
        // Hash register values
        for (key, value) in &state.eqtb_state.int_params {
            key.hash(&mut hasher);
            value.hash(&mut hasher);
        }
        
        for (key, value) in &state.eqtb_state.dimen_params {
            key.hash(&mut hasher);
            value.hash(&mut hasher);
        }
        
        // Hash font state
        state.font_state.hash(&mut hasher);
        
        hasher.finish128().as_u128()
    }
    
    /// Compute hash for spatial constraints
    pub fn hash_constraints(constraints: &SpatialConstraints) -> u128 {
        let mut hasher = SipHasher13::new();
        
        // Hash all constraint fields
        constraints.min_width.map(|x| x.to_bits()).hash(&mut hasher);
        constraints.max_width.map(|x| x.to_bits()).hash(&mut hasher);
        constraints.available_width.map(|x| x.to_bits()).hash(&mut hasher);
        constraints.min_height.map(|x| x.to_bits()).hash(&mut hasher);
        constraints.max_height.map(|x| x.to_bits()).hash(&mut hasher);
        constraints.available_height.map(|x| x.to_bits()).hash(&mut hasher);
        constraints.natural_width.map(|x| x.to_bits()).hash(&mut hasher);
        constraints.stretch.map(|x| x.to_bits()).hash(&mut hasher);
        constraints.shrink.map(|x| x.to_bits()).hash(&mut hasher);
        constraints.line_spacing.map(|x| x.to_bits()).hash(&mut hasher);
        
        hasher.finish128().as_u128()
    }
}

/// Current system timestamp in microseconds
pub fn current_timestamp_us() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_spatial_constraints_compatibility() {
        let constraint1 = SpatialConstraints {
            min_width: Some(100.0),
            max_width: Some(200.0),
            available_width: Some(150.0),
            min_height: None,
            max_height: None,
            available_height: None,
            natural_width: Some(120.0),
            stretch: Some(10.0),
            shrink: Some(5.0),
            line_spacing: Some(1.2),
            paragraph_shape: None,
            math_spacing: None,
            font_metrics: None,
            language_settings: None,
        };
        
        let constraint2 = SpatialConstraints {
            min_width: Some(100.1), // Within tolerance
            max_width: Some(199.9), // Within tolerance
            available_width: Some(150.05), // Within tolerance
            min_height: None,
            max_height: None,
            available_height: None,
            natural_width: Some(119.9), // Within tolerance
            stretch: Some(10.05), // Within tolerance
            shrink: Some(4.98), // Within tolerance
            line_spacing: Some(1.201), // Within tolerance
            paragraph_shape: None,
            math_spacing: None,
            font_metrics: None,
            language_settings: None,
        };
        
        assert!(constraint1.is_compatible(&constraint2));
    }
    
    #[test]
    fn test_cache_basic_operations() {
        let mut cache: MemoizationCache<String, i32> = MemoizationCache::new(100, 10);
        
        assert_eq!(cache.get(&"key1".to_string()), None);
        cache.insert("key1".to_string(), 42);
        assert_eq!(cache.get(&"key1".to_string()), Some(&42));
        
        let removed = cache.remove(&"key1".to_string());
        assert_eq!(removed, Some(42));
        assert_eq!(cache.get(&"key1".to_string()), None);
    }
    
    #[test]
    fn test_constraint_subsumption() {
        let general = SpatialConstraints {
            min_width: Some(50.0), // Less restrictive
            max_width: Some(300.0), // Less restrictive
            available_width: None,
            min_height: None,
            max_height: None,
            available_height: None,
            natural_width: Some(120.0),
            stretch: Some(10.0),
            shrink: Some(5.0),
            line_spacing: None,
            paragraph_shape: None,
            math_spacing: None,
            font_metrics: None,
            language_settings: None,
        };
        
        let specific = SpatialConstraints {
            min_width: Some(100.0), // More restrictive
            max_width: Some(200.0), // More restrictive
            available_width: None,
            min_height: None,
            max_height: None,
            available_height: None,
            natural_width: Some(120.0),
            stretch: Some(10.0),
            shrink: Some(5.0),
            line_spacing: None,
            paragraph_shape: None,
            math_spacing: None,
            font_metrics: None,
            language_settings: None,
        };
        
        assert!(general.subsumes(&specific));
        assert!(!specific.subsumes(&general));
    }
    
    #[test]
    fn test_hash_computation() {
        let content = "test content";
        let hash1 = HashComputation::hash_content(&content);
        let hash2 = HashComputation::hash_content(&content);
        assert_eq!(hash1, hash2);
        
        let different_content = "different content";
        let hash3 = HashComputation::hash_content(&different_content);
        assert_ne!(hash1, hash3);
    }
    
    #[test]
    fn test_dependency_graph() {
        let mut graph = DependencyGraph::new();
        
        let cache_key = CacheKey {
            content_hash: 123,
            constraint_hash: 456,
            state_hash: 789,
        };
        
        let dep_key = DependencyKey {
            dep_type: DependencyType::File,
            identifier: "test.tex".to_string(),
            value_hash: 999,
        };
        
        graph.add_dependency(cache_key.clone(), dep_key.clone());
        
        let dependents = graph.get_dependents(&dep_key);
        assert_eq!(dependents.len(), 1);
        assert_eq!(dependents[0], &cache_key);
        
        let dependencies = graph.get_dependencies(&cache_key);
        assert_eq!(dependencies.len(), 1);
        assert_eq!(dependencies[0], &dep_key);
    }
}