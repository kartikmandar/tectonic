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
use std::hash::{Hash, Hasher};
use sha2::{Sha256, Digest};
use siphasher::sip128::{Hasher128, SipHasher13 as SipHasher128};
use bincode;
#[cfg(target_arch = "wasm32")]
use js_sys;

// Import TeX engine components for hash computation
use super::TeXEngineState;

// Note: current_timestamp_us is defined elsewhere in the module

/// TeX compilation context for memoization integration
pub struct TeXCompilationContext {
    /// Current position in source document
    pub current_position: usize,
    /// Current TeX mode (horizontal, vertical, math)
    pub tex_mode: TeXMode,
    /// Current nesting level (paragraph, environment, etc.)
    pub nesting_level: u32,
    /// Available width for current context
    pub available_width: Option<f64>,
    /// Available height for current context  
    pub available_height: Option<f64>,
    /// Current font state
    pub font_properties: FontProperties,
    /// Current paragraph parameters
    pub paragraph_params: ParagraphParams,
    /// Math mode parameters (if in math mode)
    pub math_params: Option<MathParams>,
}

/// TeX compilation modes
#[derive(Debug, Clone, PartialEq)]
pub enum TeXMode {
    /// Horizontal mode (normal text)
    Horizontal,
    /// Vertical mode (page/column building)
    Vertical, 
    /// Math mode (inline or display)
    Math { display: bool },
    /// Restricted horizontal mode (inside vbox)
    RestrictedHorizontal,
    /// Internal vertical mode (inside hbox)
    InternalVertical,
}

/// Font properties for constraint generation
#[derive(Debug, Clone, PartialEq)]
pub struct FontProperties {
    pub font_id: i32,
    pub size_pt: f64,
    pub family_name: String,
    pub char_width: f64,
    pub line_height: f64,
    pub x_height: f64,
    pub baseline_skip: f64,
}

impl Hash for FontProperties {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.font_id.hash(state);
        self.size_pt.to_bits().hash(state);
        self.family_name.hash(state);
        self.char_width.to_bits().hash(state);
        self.line_height.to_bits().hash(state);
        self.x_height.to_bits().hash(state);
        self.baseline_skip.to_bits().hash(state);
    }
}

/// Paragraph formatting parameters
#[derive(Debug, Clone, PartialEq)]
pub struct ParagraphParams {
    pub line_width: f64,
    pub left_indent: f64,
    pub right_indent: f64,
    pub first_line_indent: f64,
    pub par_skip: f64,
    pub baseline_skip: f64,
    pub line_spacing: f64,
}

impl Hash for ParagraphParams {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.line_width.to_bits().hash(state);
        self.left_indent.to_bits().hash(state);
        self.right_indent.to_bits().hash(state);
        self.first_line_indent.to_bits().hash(state);
        self.par_skip.to_bits().hash(state);
        self.baseline_skip.to_bits().hash(state);
        self.line_spacing.to_bits().hash(state);
    }
}

/// Math mode parameters
#[derive(Debug, Clone, PartialEq)]
pub struct MathParams {
    pub math_surround: f64,
    pub thin_space: f64,
    pub med_space: f64,
    pub thick_space: f64,
    pub display_width: Option<f64>,
    pub display_indent: f64,
}

impl Hash for MathParams {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.math_surround.to_bits().hash(state);
        self.thin_space.to_bits().hash(state);
        self.med_space.to_bits().hash(state);
        self.thick_space.to_bits().hash(state);
        self.display_width.map(|x| x.to_bits()).hash(state);
        self.display_indent.to_bits().hash(state);
    }
}

/// Core memoization system inspired by Typst's comemo library
pub struct MemoizationSystem {
    /// Primary cache storage with hash-based keys
    cache: MemoizationCache<CacheKey, CacheEntry>,
    /// Spatial constraint index for efficient lookup
    spatial_index: SpatialIndex,
    /// Dependency graph for tracking relationships
    dependency_graph: DependencyGraph,
    /// Macro expansion tracking system
    macro_tracker: MacroTracker,
    /// Register change tracking system
    register_tracker: RegisterTracker,
    /// Cache statistics and performance metrics
    stats: CacheStatistics,
    /// Current memory usage in bytes
    current_memory_usage: usize,
    /// Memory limit in bytes
    memory_limit: usize,
}

impl MemoizationSystem {
    /// Create a new memoization system
    pub fn new(max_cache_entries: usize, memory_limit_mb: usize) -> Self {
        let memory_limit = memory_limit_mb * 1024 * 1024; // Convert MB to bytes
        Self {
            cache: MemoizationCache::new(max_cache_entries, memory_limit_mb),
            spatial_index: SpatialIndex::new(),
            dependency_graph: DependencyGraph::new(),
            macro_tracker: MacroTracker::new(),
            register_tracker: RegisterTracker::new(),
            stats: CacheStatistics::default(),
            current_memory_usage: 0,
            memory_limit,
        }
    }
    
    /// Record a macro definition for tracking
    pub fn define_macro(&mut self, name: String, definition: String, param_count: u8) {
        self.macro_tracker.define_macro(name, definition, param_count);
    }
    
    /// Begin tracking a macro expansion
    pub fn begin_macro_expansion(&mut self, macro_name: String, arguments: Vec<String>, source_position: usize) -> Option<usize> {
        self.macro_tracker.begin_expansion(macro_name, arguments, source_position)
    }
    
    /// Complete a macro expansion
    pub fn end_macro_expansion(&mut self, expansion_id: usize, expanded_content: &str) -> Option<u128> {
        self.macro_tracker.end_expansion(expansion_id, expanded_content)
    }
    
    /// Check if any dependencies have changed
    pub fn has_dependency_changes(&self) -> bool {
        self.macro_tracker.has_changes()
    }
    
    /// Get cache statistics including macro usage
    pub fn get_statistics(&self) -> (CacheStatistics, MacroStatistics) {
        (self.stats.clone(), self.macro_tracker.get_usage_statistics())
    }
    
    /// Invalidate cache entries based on changed dependencies
    pub fn invalidate_dependencies(&mut self, changed_dependencies: &[DependencyKey]) {
        for dep_key in changed_dependencies {
            let dependents = self.dependency_graph.get_dependents(dep_key);
            for cache_key in dependents {
                self.cache.remove(cache_key);
                self.stats.record_eviction();
            }
        }
    }
    
    /// Get macro state hash for cache key generation
    pub fn get_macro_state_hash(&self) -> u128 {
        self.macro_tracker.get_state_hash()
    }
    
    /// Set a count register value
    pub fn set_count_register(&mut self, register: i32, value: i32, source_position: Option<usize>) {
        self.register_tracker.set_count(register, value, source_position);
    }
    
    /// Set a dimension register value
    pub fn set_dimen_register(&mut self, register: i32, value: i32, source_position: Option<usize>) {
        self.register_tracker.set_dimen(register, value, source_position);
    }
    
    /// Set a skip register value
    pub fn set_skip_register(&mut self, register: i32, value: GlueValue, source_position: Option<usize>) {
        self.register_tracker.set_skip(register, value, source_position);
    }
    
    /// Check if any registers have changed
    pub fn has_register_changes(&self) -> bool {
        self.register_tracker.has_changes()
    }
    
    /// Get register state hash for cache key generation
    pub fn get_register_state_hash(&mut self) -> u128 {
        self.register_tracker.create_snapshot()
    }
    
    /// Get all changed dependencies (macros + registers)
    pub fn get_all_changed_dependencies(&self) -> Vec<DependencyKey> {
        let mut all_deps = Vec::new();
        
        // Add register dependencies
        all_deps.extend(self.register_tracker.get_register_dependencies());
        
        // Add macro dependencies if any macros changed
        if self.macro_tracker.has_changes() {
            // This would need to be implemented to get specific changed macros
            // For now, we'll add a general macro state dependency
        }
        
        all_deps
    }
    
    /// Check memoization cache for compatible entry using spatial constraints and content hash
    /// 
    /// This is the core cache lookup function that implements multi-level matching:
    /// 1. Exact content and constraint match (fastest path)
    /// 2. Content match with compatible constraints (constraint subsumption)
    /// 3. Content match with intersectable constraints (partial reuse)
    /// 
    /// Returns the cached result hash if found, None if cache miss
    pub fn check_memo(
        &mut self,
        content_hash: u128,
        constraints: &SpatialConstraints,
        state_hash: u128,
        source_position: usize
    ) -> Option<MemoLookupResult> {
        self.stats.total_lookups += 1;
        
        // Create cache key for lookup
        let cache_key = CacheKey {
            content_hash,
            constraint_hash: HashComputation::hash_constraints(constraints),
            state_hash,
        };
        
        // Level 1: Exact match lookup (O(1) hash table lookup)
        if let Some(exact_entry) = self.cache.get(&cache_key) {
            let result = MemoLookupResult {
                result_hash: exact_entry.result_hash,
                match_type: CacheMatchType::Exact,
                constraints_satisfied: exact_entry.constraints.clone(),
                dependencies: exact_entry.dependencies.clone(),
                metadata: exact_entry.metadata.clone(),
            };
            
            self.stats.record_hit();
            self.update_access_statistics(&cache_key);
            
            return Some(result);
        }
        
        // Level 2: Content match with compatible constraints
        // Use spatial index to find entries with same content but different constraints
        let content_matches = self.find_content_matches(content_hash, state_hash);
        
        for candidate_key in content_matches {
            if let Some(candidate_entry) = self.cache.get(&candidate_key) {
                // Check if candidate constraints are compatible with our requirements
                if let Some(compatibility) = self.check_constraint_compatibility(
                    constraints, 
                    &candidate_entry.constraints
                ) {
                    let result = MemoLookupResult {
                        result_hash: candidate_entry.result_hash,
                        match_type: compatibility.match_type,
                        constraints_satisfied: compatibility.effective_constraints,
                        dependencies: candidate_entry.dependencies.clone(),
                        metadata: candidate_entry.metadata.clone(),
                    };
                    
                    self.stats.record_hit();
                    self.update_access_statistics(&candidate_key);
                    
                    return Some(result);
                }
            }
        }
        
        // Level 3: Dependency-aware lookup
        // Check if any cached entry with different content could be reused
        // if dependencies haven't changed
        if let Some(dependency_match) = self.check_dependency_based_reuse(
            content_hash,
            constraints,
            state_hash,
            source_position
        ) {
            self.stats.record_hit();
            return Some(dependency_match);
        }
        
        // Cache miss - record statistics and return None
        self.stats.record_miss();
        self.record_cache_miss_reason(content_hash, constraints, state_hash);
        
        None
    }
    
    /// Store a computation result in the memoization cache
    /// This function is called after successful compilation to cache the result
    pub fn store_memo(
        &mut self,
        content_hash: u128,
        constraints: &SpatialConstraints,
        state_hash: u128,
        result_hash: u128,
        source_position: usize,
        computation_time_us: u64,
        memory_used_bytes: usize,
        tex_operations: u32,
        dependencies: Vec<DependencyKey>
    ) -> Result<(), String> {
        // Check memory limits before storing
        let estimated_entry_size = std::mem::size_of::<CacheEntry>() + 
                                   std::mem::size_of::<SpatialConstraints>() +
                                   dependencies.len() * std::mem::size_of::<DependencyKey>() +
                                   memory_used_bytes;
        
        if self.current_memory_usage + estimated_entry_size > self.memory_limit {
            // Trigger eviction to make room
            self.evict_entries_for_space(estimated_entry_size)?;
        }
        
        // Create cache key
        let cache_key = CacheKey {
            content_hash,
            constraint_hash: HashComputation::hash_constraints(constraints),
            state_hash,
        };
        
        // Create cache entry
        let entry = CacheEntry {
            result_hash,
            constraints: constraints.clone(),
            dependencies: dependencies.clone(),
            metadata: CacheMetadata {
                source_position,
                context_type: "tex_compilation".to_string(),
                result_size_bytes: memory_used_bytes,
                metrics: EntryMetrics {
                    computation_time_us,
                    memory_used_bytes,
                    tex_operations,
                    hit_rate: 0.0, // Will be updated based on usage
                },
            },
            timestamp: current_timestamp_us(),
            access_count: 0,
            last_access: current_timestamp_us(),
        };
        
        // Store in cache
        self.cache.insert(cache_key.clone(), entry)?;
        
        // Update spatial index
        self.spatial_index.add_entry(cache_key.clone(), constraints);
        
        // Update dependency graph
        for dep in &dependencies {
            self.dependency_graph.add_dependency(dep.clone(), cache_key.clone());
        }
        
        // Update memory usage
        self.current_memory_usage += estimated_entry_size;
        
        // Update statistics
        self.stats.record_store();
        
        Ok(())
    }
    
    /// Evict cache entries to make room for new entries
    fn evict_entries_for_space(&mut self, required_space: usize) -> Result<(), String> {
        let mut freed_space = 0;
        let mut entries_to_evict = Vec::new();
        
        // Sort entries by LRU (least recently used first)
        let mut sorted_entries: Vec<_> = self.cache.storage.iter()
            .map(|(k, v)| (k.clone(), v.last_access, v.metadata.result_size_bytes))
            .collect();
        sorted_entries.sort_by_key(|(_, last_access, _)| *last_access);
        
        // Select entries to evict
        for (key, _, size) in sorted_entries {
            if freed_space >= required_space {
                break;
            }
            entries_to_evict.push(key);
            freed_space += size;
        }
        
        // Evict selected entries
        for key in entries_to_evict {
            self.remove_entry(&key);
        }
        
        if freed_space < required_space {
            return Err(format!("Unable to free enough space. Required: {}, Freed: {}", 
                             required_space, freed_space));
        }
        
        Ok(())
    }
    
    /// Remove a cache entry and clean up associated data
    fn remove_entry(&mut self, key: &CacheKey) {
        if let Some(entry) = self.cache.remove(key) {
            // Update memory usage
            self.current_memory_usage -= entry.metadata.result_size_bytes;
            
            // Remove from spatial index
            self.spatial_index.remove_entry(key);
            
            // Remove from dependency graph
            for dep in &entry.dependencies {
                self.dependency_graph.remove_dependency(dep, key);
            }
            
            // Update statistics
            self.stats.record_eviction();
        }
    }
    
    /// Find all cache entries with matching content hash and state hash
    /// Uses spatial index for efficient constraint-based lookup
    fn find_content_matches(&self, content_hash: u128, state_hash: u128) -> Vec<CacheKey> {
        // This would be more efficient with a proper content index
        // For now, we'll iterate through spatial index entries
        let mut matches = Vec::new();
        
        // Use spatial index to narrow down candidates
        for (spatial_key, cache_keys) in &self.spatial_index.constraint_map {
            for cache_key in cache_keys {
                if cache_key.content_hash == content_hash && cache_key.state_hash == state_hash {
                    matches.push(cache_key.clone());
                }
            }
        }
        
        matches
    }
    
    /// Check if cached constraints are compatible with required constraints
    /// Returns compatibility info if reusable, None if incompatible
    fn check_constraint_compatibility(
        &self,
        required: &SpatialConstraints,
        cached: &SpatialConstraints,
    ) -> Option<ConstraintCompatibility> {
        // Exact match (best case)
        if required == cached {
            return Some(ConstraintCompatibility {
                match_type: CacheMatchType::Exact,
                effective_constraints: required.clone(),
                compatibility_score: 1.0,
            });
        }
        
        // Check if cached constraints subsume required constraints
        if cached.subsumes(required) {
            return Some(ConstraintCompatibility {
                match_type: CacheMatchType::Subsumption,
                effective_constraints: required.clone(),
                compatibility_score: 0.9,
            });
        }
        
        // Check if required constraints subsume cached constraints
        if required.subsumes(cached) {
            return Some(ConstraintCompatibility {
                match_type: CacheMatchType::Subsumption,
                effective_constraints: cached.clone(),
                compatibility_score: 0.8,
            });
        }
        
        // Check if constraints are compatible within tolerance
        if required.is_compatible(cached) {
            let intersection = required.intersect(cached);
            return Some(ConstraintCompatibility {
                match_type: CacheMatchType::Compatible,
                effective_constraints: intersection,
                compatibility_score: self.calculate_compatibility_score(required, cached),
            });
        }
        
        // Incompatible constraints
        None
    }
    
    /// Calculate compatibility score between two constraint sets
    /// Returns value between 0.0 (incompatible) and 1.0 (identical)
    fn calculate_compatibility_score(
        &self,
        required: &SpatialConstraints,
        cached: &SpatialConstraints,
    ) -> f64 {
        let mut score = 0.0;
        let mut components = 0;
        
        // Width compatibility
        if let (Some(req_min), Some(cached_min)) = (required.min_width, cached.min_width) {
            score += 1.0 - (req_min - cached_min).abs() / req_min.max(cached_min);
            components += 1;
        }
        
        if let (Some(req_max), Some(cached_max)) = (required.max_width, cached.max_width) {
            score += 1.0 - (req_max - cached_max).abs() / req_max.max(cached_max);
            components += 1;
        }
        
        // Height compatibility
        if let (Some(req_min), Some(cached_min)) = (required.min_height, cached.min_height) {
            score += 1.0 - (req_min - cached_min).abs() / req_min.max(cached_min);
            components += 1;
        }
        
        if let (Some(req_max), Some(cached_max)) = (required.max_height, cached.max_height) {
            score += 1.0 - (req_max - cached_max).abs() / req_max.max(cached_max);
            components += 1;
        }
        
        // Box constraints
        if let (Some(req_width), Some(cached_width)) = (required.natural_width, cached.natural_width) {
            score += 1.0 - (req_width - cached_width).abs() / req_width.max(cached_width);
            components += 1;
        }
        
        // Return average score, or 0.5 if no comparable components
        if components > 0 {
            score / components as f64
        } else {
            0.5
        }
    }
    
    /// Check for dependency-based cache reuse opportunities
    /// This handles cases where content differs but dependencies are stable
    fn check_dependency_based_reuse(
        &self,
        content_hash: u128,
        constraints: &SpatialConstraints,
        state_hash: u128,
        source_position: usize,
    ) -> Option<MemoLookupResult> {
        // Check if we have any entries with same dependencies but different content
        // This is useful for cases where only whitespace or comments changed
        
        // For now, we'll implement a simple version that checks recent entries
        // A full implementation would maintain a dependency-to-cache mapping
        
        // This is a placeholder for more sophisticated dependency-based matching
        // that would be implemented with full dependency graph traversal
        None
    }
    
    /// Update access statistics for cache entry (for LRU and performance tracking)
    fn update_access_statistics(&mut self, key: &CacheKey) {
        // This would update LRU information in a full implementation
        // For now, we'll just record the access
        if let Some(cached_entry) = self.cache.storage.get_mut(key) {
            cached_entry.access_count += 1;
            cached_entry.last_access = current_timestamp_us();
        }
    }
    
    /// Record detailed information about cache misses for optimization
    fn record_cache_miss_reason(
        &mut self,
        content_hash: u128,
        constraints: &SpatialConstraints,
        state_hash: u128,
    ) {
        // In a full implementation, this would maintain detailed miss statistics
        // to help with cache optimization and debugging
        
        // For now, we'll record basic miss information
        self.stats.timing_stats.total_cache_time_us += 10; // Estimated miss overhead
    }
    
    /// Get comprehensive statistics
    pub fn get_comprehensive_statistics(&self) -> (CacheStatistics, MacroStatistics, RegisterStatistics) {
        (
            self.stats.clone(),
            self.macro_tracker.get_usage_statistics(),
            self.register_tracker.get_statistics()
        )
    }
    
    // =============================================================================
    // TeX Integration Methods for Box Building and Paragraph Construction
    // =============================================================================
    
    /// Check memoization cache before TeX box building
    /// This function integrates constraint compatibility checking into TeX's compilation pipeline
    pub fn check_box_memo(
        &mut self,
        tex_content: &str,
        context: &TeXCompilationContext
    ) -> Option<MemoLookupResult> {
        // Generate content hash from TeX source content
        let content_hash = HashComputation::hash_tex_source(tex_content);
        
        // Generate spatial constraints from current TeX state
        let constraints = self.generate_spatial_constraints_from_context(context);
        
        // Generate comprehensive state hash including all tracked state
        let state_hash = self.generate_comprehensive_state_hash();
        
        // Perform memoization check
        self.check_memo(content_hash, &constraints, state_hash, context.current_position)
    }
    
    /// Check memoization cache before paragraph construction
    /// Specifically optimized for paragraph-level caching with line-breaking constraints
    pub fn check_paragraph_memo(
        &mut self,
        paragraph_content: &str,
        context: &TeXCompilationContext,
        par_params: &ParagraphParams
    ) -> Option<MemoLookupResult> {
        // Generate paragraph-specific content hash
        let content_hash = HashComputation::hash_paragraph_content(
            paragraph_content,
            par_params
        );
        
        // Generate paragraph-specific spatial constraints
        let constraints = self.generate_paragraph_constraints(context, par_params);
        
        // Include paragraph state in hash
        let state_hash = self.generate_paragraph_state_hash(par_params);
        
        self.check_memo(content_hash, &constraints, state_hash, context.current_position)
    }
    
    /// Check memoization cache before math mode compilation
    /// Handles both inline and display math with appropriate constraints
    pub fn check_math_memo(
        &mut self,
        math_content: &str,
        context: &TeXCompilationContext,
        is_display: bool
    ) -> Option<MemoLookupResult> {
        // Generate math-specific content hash
        let content_hash = HashComputation::hash_math_content(math_content, is_display);
        
        // Generate math-specific spatial constraints
        let constraints = self.generate_math_constraints(context, is_display);
        
        // Include math state in hash
        let state_hash = self.generate_math_state_hash(&context.math_params, is_display);
        
        self.check_memo(content_hash, &constraints, state_hash, context.current_position)
    }
    
    /// Generate spatial constraints from current TeX compilation context
    fn generate_spatial_constraints_from_context(
        &self,
        context: &TeXCompilationContext
    ) -> SpatialConstraints {
        SpatialConstraints {
            min_width: None,
            max_width: context.available_width,
            available_width: context.available_width,
            min_height: None,
            max_height: context.available_height,
            available_height: context.available_height,
            natural_width: None,
            stretch: None,
            shrink: None,
            line_spacing: Some(context.paragraph_params.line_spacing),
            paragraph_shape: Some(ParagraphShape {
                left_indent: context.paragraph_params.left_indent,
                right_indent: context.paragraph_params.right_indent,
                first_line_indent: context.paragraph_params.first_line_indent,
                line_width: context.paragraph_params.line_width,
                hanging_indent: 0.0, // Would need to be tracked separately
            }),
            math_spacing: context.math_params.as_ref().map(|mp| MathSpacing {
                thin_space: mp.thin_space,
                med_space: mp.med_space,
                thick_space: mp.thick_space,
                quad_space: mp.thick_space * 1.2, // Approximate
            }),
            font_metrics: Some(FontMetrics {
                size: context.font_properties.size_pt,
                family: context.font_properties.family_name.clone(),
                char_width: Some(context.font_properties.char_width),
                line_height: context.font_properties.line_height,
                x_height: context.font_properties.x_height,
                baseline_skip: context.font_properties.baseline_skip,
            }),
            language_settings: Some(LanguageSettings {
                language: "en".to_string(), // Would need to be tracked from TeX state
                hyphenation: true,
                ltr: true,
            }),
        }
    }
    
    /// Generate paragraph-specific spatial constraints
    fn generate_paragraph_constraints(
        &self,
        context: &TeXCompilationContext,
        par_params: &ParagraphParams
    ) -> SpatialConstraints {
        SpatialConstraints {
            min_width: Some(par_params.line_width * 0.5), // Minimum reasonable line width
            max_width: Some(par_params.line_width),
            available_width: Some(par_params.line_width),
            min_height: Some(par_params.baseline_skip),
            max_height: None, // Paragraph can grow vertically
            available_height: context.available_height,
            natural_width: Some(par_params.line_width),
            stretch: Some(par_params.line_width * 0.1), // 10% stretch allowance
            shrink: Some(par_params.line_width * 0.05),  // 5% shrink allowance
            line_spacing: Some(par_params.line_spacing),
            paragraph_shape: Some(ParagraphShape {
                left_indent: par_params.left_indent,
                right_indent: par_params.right_indent,
                first_line_indent: par_params.first_line_indent,
                line_width: par_params.line_width,
                hanging_indent: 0.0,
            }),
            math_spacing: None, // Not relevant for paragraphs
            font_metrics: Some(FontMetrics {
                size: context.font_properties.size_pt,
                family: context.font_properties.family_name.clone(),
                char_width: Some(context.font_properties.char_width),
                line_height: context.font_properties.line_height,
                x_height: context.font_properties.x_height,
                baseline_skip: context.font_properties.baseline_skip,
            }),
            language_settings: Some(LanguageSettings {
                language: "en".to_string(),
                hyphenation: true,
                ltr: true,
            }),
        }
    }
    
    /// Generate math-specific spatial constraints
    fn generate_math_constraints(
        &self,
        context: &TeXCompilationContext,
        is_display: bool
    ) -> SpatialConstraints {
        let math_params = context.math_params.as_ref();
        
        SpatialConstraints {
            min_width: if is_display { 
                math_params.and_then(|mp| mp.display_width.map(|w| w * 0.5))
            } else { 
                Some(context.font_properties.char_width * 2.0) // At least 2 characters wide
            },
            max_width: if is_display {
                math_params.and_then(|mp| mp.display_width)
            } else {
                context.available_width
            },
            available_width: if is_display {
                math_params.and_then(|mp| mp.display_width)
            } else {
                context.available_width
            },
            min_height: Some(context.font_properties.x_height),
            max_height: None, // Math can grow vertically
            available_height: context.available_height,
            natural_width: None, // Math width depends on content
            stretch: None, // Math expressions don't stretch like text
            shrink: None,  // Math expressions don't shrink like text
            line_spacing: None, // Not applicable to math
            paragraph_shape: None, // Not applicable to math
            math_spacing: math_params.map(|mp| MathSpacing {
                thin_space: mp.thin_space,
                med_space: mp.med_space,
                thick_space: mp.thick_space,
                quad_space: mp.thick_space * 1.2,
            }),
            font_metrics: Some(FontMetrics {
                size: context.font_properties.size_pt,
                family: context.font_properties.family_name.clone(),
                char_width: Some(context.font_properties.char_width),
                line_height: context.font_properties.line_height,
                x_height: context.font_properties.x_height,
                baseline_skip: context.font_properties.baseline_skip,
            }),
            language_settings: None, // Language settings don't apply to math
        }
    }
    
    /// Generate comprehensive state hash including all tracked TeX state
    fn generate_comprehensive_state_hash(&mut self) -> u128 {
        let mut hasher = SipHasher128::new();
        
        // Include macro state
        self.get_macro_state_hash().hash(&mut hasher);
        
        // Include register state
        self.get_register_state_hash().hash(&mut hasher);
        
        // Include any other tracked state (fonts, packages, etc.)
        // This would be expanded as more state tracking is added
        
        hasher.finish128()
    }
    
    /// Generate paragraph-specific state hash
    fn generate_paragraph_state_hash(&mut self, par_params: &ParagraphParams) -> u128 {
        let mut hasher = SipHasher128::new();
        
        // Include general state
        self.generate_comprehensive_state_hash().hash(&mut hasher);
        
        // Include paragraph-specific parameters
        HashComputation::hash_content(par_params).hash(&mut hasher);
        
        hasher.finish128()
    }
    
    /// Generate math-specific state hash
    fn generate_math_state_hash(
        &mut self,
        math_params: &Option<MathParams>,
        is_display: bool
    ) -> u128 {
        let mut hasher = SipHasher128::new();
        
        // Include general state
        self.generate_comprehensive_state_hash().hash(&mut hasher);
        
        // Include math-specific parameters
        if let Some(mp) = math_params {
            HashComputation::hash_content(mp).hash(&mut hasher);
        }
        is_display.hash(&mut hasher);
        
        hasher.finish128()
    }
    
    /// Update constraint compatibility checking based on TeX's box building state
    /// This method should be called during TeX's box building routines
    pub fn update_box_building_context(
        &mut self,
        _current_hsize: f64,
        _current_vsize: f64,
        _current_font: i32,
        _baseline_skip: f64
    ) {
        // This would update internal state that affects constraint generation
        // For now, we'll just record these for potential constraint updates
        
        // In a full implementation, this would:
        // 1. Update available width/height context
        // 2. Track current font for constraint generation
        // 3. Update spacing parameters
        // 4. Possibly trigger cache invalidation if significant changes occur
    }
    
    /// Notify memoization system of paragraph break
    /// This helps segment memoization at natural TeX boundaries
    pub fn notify_paragraph_break(&mut self, _position: usize) {
        // Mark a natural memoization boundary
        // This could be used to optimize cache segmentation
        // and improve cache hit rates by caching at paragraph boundaries
    }
    
    /// Notify memoization system of page break
    /// This helps manage cache scope and memory usage
    pub fn notify_page_break(&mut self, _page_number: i32, _position: usize) {
        // Mark page boundaries for cache management
        // This could trigger memory cleanup for earlier pages
        // and help maintain the 400MB memory target
    }
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
    
    /// Store a memoization result in the cache with full dependency tracking
    /// This implements the complete storage system for Phase 2.3
    pub fn store_memo(
        &mut self,
        content_hash: u128,
        constraints: &SpatialConstraints,
        state_hash: u128,
        result_hash: u128,
        dependencies: Vec<DependencyKey>,
        source_position: usize,
        computation_metrics: EntryMetrics
    ) -> Result<(), MemoizationError> {
        // Check memory limits before storing
        let estimated_entry_size = self.estimate_entry_size(&dependencies, &computation_metrics);
        if self.would_exceed_memory_limit(estimated_entry_size) {
            self.perform_lru_eviction(estimated_entry_size)?;
        }
        
        // Create cache key
        let cache_key = CacheKey {
            content_hash,
            constraint_hash: HashComputation::hash_constraints(constraints),
            state_hash,
        };
        
        // Create cache entry
        let cache_entry = CacheEntry {
            result_hash,
            constraints: constraints.clone(),
            dependencies: dependencies.clone(),
            metadata: CacheMetadata {
                source_position,
                context_type: self.determine_context_type(source_position),
                result_size_bytes: estimated_entry_size,
                metrics: computation_metrics,
            },
            timestamp: current_timestamp_us(),
            access_count: 1, // First access is the store operation
            last_access: current_timestamp_us(),
        };
        
        // Store in cache
        self.cache.insert(cache_key.clone(), cache_entry);
        
        // Update dependency tracking
        for dep in dependencies {
            self.dependency_graph.add_dependency(cache_key.clone(), dep);
        }
        
        // Update cache size tracking
        self.current_memory_usage += estimated_entry_size;
        
        // Update statistics
        self.stats.total_entries += 1;
        self.stats.current_size = self.cache.len();
        
        Ok(())
    }
    
    /// Estimate memory size of a cache entry for memory management
    fn estimate_entry_size(&self, dependencies: &[DependencyKey], metrics: &EntryMetrics) -> usize {
        let base_size = std::mem::size_of::<CacheEntry>();
        let dependencies_size = dependencies.len() * std::mem::size_of::<DependencyKey>();
        let metadata_size = metrics.memory_used_bytes;
        
        base_size + dependencies_size + metadata_size
    }
    
    /// Check if storing this entry would exceed memory limits
    fn would_exceed_memory_limit(&self, entry_size: usize) -> bool {
        self.current_memory_usage + entry_size > self.memory_limit
    }
    
    /// Perform LRU eviction to make space for new entries
    fn perform_lru_eviction(&mut self, needed_space: usize) -> Result<(), MemoizationError> {
        let mut freed_space = 0;
        let mut candidates: Vec<_> = self.cache.iter()
            .map(|(key, entry)| (key.clone(), entry.last_access, entry.metadata.result_size_bytes))
            .collect();
        
        // Sort by last access time (oldest first)
        candidates.sort_by_key(|(_, last_access, _)| *last_access);
        
        for (key, _, size) in candidates {
            if freed_space >= needed_space {
                break;
            }
            
            // Remove from cache
            if let Some(removed_entry) = self.cache.remove(&key) {
                // Update dependency graph
                for dep in &removed_entry.dependencies {
                    self.dependency_graph.remove_dependency(&key, dep);
                }
                
                freed_space += size;
                self.current_memory_usage = self.current_memory_usage.saturating_sub(size);
                self.stats.evictions += 1;
            }
        }
        
        if freed_space < needed_space {
            return Err(MemoizationError::InsufficientMemory {
                needed: needed_space,
                available: freed_space,
            });
        }
        
        Ok(())
    }
    
    /// Determine context type from source position for debugging
    fn determine_context_type(&self, _source_position: usize) -> String {
        // This would integrate with actual TeX parser to determine context
        // For now, return a placeholder
        "paragraph".to_string()
    }
    
    /// Add cache hit statistics tracking
    fn update_cache_hit_statistics(&mut self, cache_key: &CacheKey, match_type: &CacheMatchType) {
        if let Some(entry) = self.cache.get_mut(cache_key) {
            entry.access_count += 1;
            entry.last_access = current_timestamp_us();
            
            // Update hit rate for this specific pattern
            let total_accesses = entry.access_count;
            let hits = match match_type {
                CacheMatchType::Exact => total_accesses,
                CacheMatchType::Compatible => total_accesses - 1, // One miss that led to compatibility
                CacheMatchType::Partial => total_accesses - 2,    // More misses before partial match
            };
            entry.metadata.metrics.hit_rate = hits as f64 / total_accesses as f64;
        }
        
        // Update global statistics
        match match_type {
            CacheMatchType::Exact => self.stats.exact_hits += 1,
            CacheMatchType::Compatible => self.stats.compatible_hits += 1,
            CacheMatchType::Partial => self.stats.partial_hits += 1,
        }
    }
    
    /// Debug logging for cache behavior (Phase 2.3 requirement)
    pub fn log_cache_behavior(&self, operation: &str, cache_key: &CacheKey, details: &str) {
        #[cfg(target_arch = "wasm32")]
        {
            use web_sys::console;
            console::log_1(
                &format!(
                    "[MemoCache] {} - Key: {:x} - {}",
                    operation,
                    cache_key.content_hash,
                    details
                ).into()
            );
        }
        
        #[cfg(not(target_arch = "wasm32"))]
        {
            println!(
                "[MemoCache] {} - Key: {:x} - {}",
                operation,
                cache_key.content_hash,
                details
            );
        }
    }
    
    /// Get comprehensive cache statistics
    pub fn get_cache_statistics(&self) -> CacheStatistics {
        let hit_rate = if self.stats.total_lookups > 0 {
            (self.stats.exact_hits + self.stats.compatible_hits + self.stats.partial_hits) as f64 
                / self.stats.total_lookups as f64
        } else {
            0.0
        };
        
        CacheStatistics {
            total_entries: self.stats.total_entries,
            current_size: self.stats.current_size,
            total_lookups: self.stats.total_lookups,
            exact_hits: self.stats.exact_hits,
            compatible_hits: self.stats.compatible_hits,
            partial_hits: self.stats.partial_hits,
            misses: self.stats.total_lookups - self.stats.exact_hits - self.stats.compatible_hits - self.stats.partial_hits,
            hit_rate,
            memory_usage_bytes: self.current_memory_usage,
            memory_limit_bytes: self.memory_limit,
            evictions: self.stats.evictions,
        }
    }
}

/// Error types for memoization operations
#[derive(Debug)]
pub enum MemoizationError {
    InsufficientMemory { needed: usize, available: usize },
    InvalidConstraints { reason: String },
    DependencyError { message: String },
}

/// Comprehensive cache statistics for monitoring
#[derive(Debug, Clone, Default)]
pub struct CacheStatistics {
    pub total_entries: usize,
    pub current_size: usize,
    pub total_lookups: usize,
    pub exact_hits: usize,
    pub compatible_hits: usize,
    pub partial_hits: usize,
    pub misses: usize,
    pub hit_rate: f64,
    pub memory_usage_bytes: usize,
    pub memory_limit_bytes: usize,
    pub evictions: usize,
    pub stores: usize,
}

impl CacheStatistics {
    /// Record a cache hit
    pub fn record_hit(&mut self) {
        self.total_lookups += 1;
        self.exact_hits += 1;
        self.update_hit_rate();
    }
    
    /// Record a cache miss
    pub fn record_miss(&mut self) {
        self.total_lookups += 1;
        self.misses += 1;
        self.update_hit_rate();
    }
    
    /// Record an eviction
    pub fn record_eviction(&mut self) {
        self.evictions += 1;
        self.total_entries = self.total_entries.saturating_sub(1);
    }
    
    /// Record a cache store
    pub fn record_store(&mut self) {
        self.stores += 1;
        self.total_entries += 1;
    }
    
    /// Update hit rate calculation
    fn update_hit_rate(&mut self) {
        if self.total_lookups > 0 {
            let total_hits = self.exact_hits + self.compatible_hits + self.partial_hits;
            self.hit_rate = total_hits as f64 / self.total_lookups as f64;
        }
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

/// Hash computation utilities using hybrid SHA-256 + SipHash approach
pub struct HashComputation;

impl HashComputation {
    /// Compute 128-bit SipHash for cache keys (fast, for hashable types)
    pub fn hash_content<T: Hash>(content: &T) -> u128 {
        let mut hasher = SipHasher13::new();
        content.hash(&mut hasher);
        hasher.finish128().as_u128()
    }
    
    /// Compute comprehensive hash for TeX content with macro expansion tracking
    /// This is the main entry point for cache key generation during compilation
    pub fn compute_tex_content_hash(
        content: &str,
        macro_tracker: &MacroTracker,
        font_state: &crate::wasm::FontState,
        counter_states: &HashMap<String, i32>,
        register_state_hash: u128
    ) -> u128 {
        let mut hasher = Sha256::new();
        
        // Hash the primary content
        hasher.update(content.as_bytes());
        
        // Include macro expansion state
        let macro_state_hash = macro_tracker.get_state_hash();
        hasher.update(&macro_state_hash.to_be_bytes());
        
        // Include font state
        let font_hash = Self::hash_font_state(font_state);
        hasher.update(&font_hash.to_be_bytes());
        
        // Include counter states that affect output
        let counter_hash = Self::hash_counter_states(counter_states, &HashMap::new(), &HashMap::new());
        hasher.update(&counter_hash.to_be_bytes());
        
        // Include register state
        hasher.update(&register_state_hash.to_be_bytes());
        
        let result = hasher.finalize();
        let mut bytes = [0u8; 16];
        bytes.copy_from_slice(&result[0..16]);
        u128::from_be_bytes(bytes)
    }
    
    /// Compute SHA-256 hash for TeX source content (secure, for text content)
    pub fn hash_tex_source(source: &str) -> u128 {
        let mut hasher = Sha256::new();
        hasher.update(source.as_bytes());
        let result = hasher.finalize();
        
        // Convert first 16 bytes of SHA-256 to u128
        let mut bytes = [0u8; 16];
        bytes.copy_from_slice(&result[0..16]);
        u128::from_be_bytes(bytes)
    }
    
    /// Compute hybrid hash combining SHA-256 content hash with SipHash state hash
    /// This provides both content security and fast structural hashing
    pub fn hash_tex_content_with_state(source: &str, state_hash: u128) -> u128 {
        let content_hash = Self::hash_tex_source(source);
        
        // Combine content and state hashes using SipHash for final result
        let mut hasher = SipHasher13::new();
        content_hash.hash(&mut hasher);
        state_hash.hash(&mut hasher);
        hasher.finish128().as_u128()
    }
    
    /// Hash TeX macro definitions and expansions
    pub fn hash_macro_definitions(macros: &HashMap<String, String>) -> u128 {
        let mut hasher = Sha256::new();
        
        // Sort macros by name for deterministic hashing
        let mut sorted_macros: Vec<_> = macros.iter().collect();
        sorted_macros.sort_by_key(|(name, _)| *name);
        
        for (name, definition) in sorted_macros {
            hasher.update(name.as_bytes());
            hasher.update(b"=");
            hasher.update(definition.as_bytes());
            hasher.update(b";");
        }
        
        let result = hasher.finalize();
        let mut bytes = [0u8; 16];
        bytes.copy_from_slice(&result[0..16]);
        u128::from_be_bytes(bytes)
    }
    
    /// Hash TeX register states (counters, dimensions, etc.)
    pub fn hash_register_state(
        int_params: &HashMap<String, i32>,
        dimen_params: &HashMap<String, i32>
    ) -> u128 {
        let mut hasher = SipHasher13::new();
        
        // Sort and hash integer parameters
        let mut sorted_ints: Vec<_> = int_params.iter().collect();
        sorted_ints.sort_by_key(|(name, _)| *name);
        for (name, value) in sorted_ints {
            name.hash(&mut hasher);
            value.hash(&mut hasher);
        }
        
        // Sort and hash dimension parameters  
        let mut sorted_dimens: Vec<_> = dimen_params.iter().collect();
        sorted_dimens.sort_by_key(|(name, _)| *name);
        for (name, value) in sorted_dimens {
            name.hash(&mut hasher);
            value.hash(&mut hasher);
        }
        
        hasher.finish128().as_u128()
    }
    
    /// Hash font state from Phase 2.1 system
    pub fn hash_font_state(font_state: &crate::wasm::FontState) -> u128 {
        let mut hasher = SipHasher13::new();
        
        // Hash current font settings
        font_state.cur_f.hash(&mut hasher);
        font_state.cur_c.hash(&mut hasher);
        font_state.cur_size.hash(&mut hasher);
        
        // Hash font metrics tables - use checksums for large arrays
        let char_base_checksum = Self::hash_i32_array_checksum(&font_state.char_base);
        let width_base_checksum = Self::hash_i32_array_checksum(&font_state.width_base);
        let height_base_checksum = Self::hash_i32_array_checksum(&font_state.height_base);
        let depth_base_checksum = Self::hash_i32_array_checksum(&font_state.depth_base);
        let italic_base_checksum = Self::hash_i32_array_checksum(&font_state.italic_base);
        let param_base_checksum = Self::hash_i32_array_checksum(&font_state.param_base);
        
        char_base_checksum.hash(&mut hasher);
        width_base_checksum.hash(&mut hasher);
        height_base_checksum.hash(&mut hasher);
        depth_base_checksum.hash(&mut hasher);
        italic_base_checksum.hash(&mut hasher);
        param_base_checksum.hash(&mut hasher);
        
        // Hash font loading state
        font_state.font_mem_size.hash(&mut hasher);
        font_state.font_max.hash(&mut hasher);
        font_state.fmem_ptr.hash(&mut hasher);
        
        hasher.finish128().as_u128()
    }
    
    /// Fast checksum for large integer arrays
    fn hash_i32_array_checksum(array: &[i32]) -> u64 {
        let mut checksum: u64 = 0;
        for &value in array.iter().take(1000) { // Sample first 1000 elements for performance
            checksum = checksum.wrapping_mul(31).wrapping_add(value as u64);
        }
        // Include array length to differentiate arrays of different sizes
        checksum = checksum.wrapping_mul(17).wrapping_add(array.len() as u64);
        checksum
    }
    
    /// Hash font metrics with spatial layout constraints
    pub fn hash_font_metrics_with_constraints(
        font_state: &crate::wasm::FontState,
        constraints: &SpatialConstraints
    ) -> u128 {
        let font_hash = Self::hash_font_state(font_state);
        let constraint_hash = Self::hash_constraints(constraints);
        
        let mut hasher = SipHasher13::new();
        font_hash.hash(&mut hasher);
        constraint_hash.hash(&mut hasher);
        hasher.finish128().as_u128()
    }
    
    /// Create font-aware dependency key for cache invalidation
    pub fn create_font_dependency(font_name: &str, font_size: f64, font_metrics_hash: u128) -> DependencyKey {
        DependencyKey {
            dep_type: DependencyType::Font,
            identifier: format!("{}@{:.1}pt", font_name, font_size),
            value_hash: font_metrics_hash,
        }
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
        
        // Hash font state using specialized function
        let font_hash = Self::hash_font_state(&state.font_state);
        font_hash.hash(&mut hasher);
        
        hasher.finish128().as_u128()
    }
    
    /// Incremental hash updating - only recompute changed components
    pub fn update_incremental_hash(
        base_hash: u128,
        changed_component: &str,
        new_value_hash: u128
    ) -> u128 {
        let mut hasher = SipHasher13::new();
        base_hash.hash(&mut hasher);
        changed_component.hash(&mut hasher);
        new_value_hash.hash(&mut hasher);
        hasher.finish128().as_u128()
    }
    
    /// Hash file content with modification time for external dependency tracking
    pub fn hash_file_fingerprint(file_path: &str, content: &[u8], modified_time: u64) -> u128 {
        let mut hasher = Sha256::new();
        hasher.update(file_path.as_bytes());
        hasher.update(&modified_time.to_be_bytes());
        hasher.update(content);
        
        let result = hasher.finalize();
        let mut bytes = [0u8; 16];
        bytes.copy_from_slice(&result[0..16]);
        u128::from_be_bytes(bytes)
    }
    
    /// Hash package loading state for dependency tracking
    pub fn hash_package_state(packages: &[String], package_versions: &HashMap<String, String>) -> u128 {
        let mut hasher = Sha256::new();
        
        // Sort packages for deterministic hashing
        let mut sorted_packages = packages.to_vec();
        sorted_packages.sort();
        
        for package in sorted_packages {
            hasher.update(package.as_bytes());
            if let Some(version) = package_versions.get(&package) {
                hasher.update(b"@");
                hasher.update(version.as_bytes());
            }
            hasher.update(b";");
        }
        
        let result = hasher.finalize();
        let mut bytes = [0u8; 16];
        bytes.copy_from_slice(&result[0..16]);
        u128::from_be_bytes(bytes)
    }
    
    /// Hash macro expansion tracking with depth and context
    /// This tracks the full expansion tree for proper cache invalidation
    pub fn hash_macro_expansion_tracking(
        macro_name: &str,
        expansion_depth: u32,
        expanded_content: &str,
        context_vars: &HashMap<String, String>
    ) -> u128 {
        let mut hasher = Sha256::new();
        
        // Hash macro identification
        hasher.update(b"MACRO:");
        hasher.update(macro_name.as_bytes());
        hasher.update(b"@DEPTH:");
        hasher.update(&expansion_depth.to_be_bytes());
        
        // Hash expanded content (critical for cache validation)
        hasher.update(b"@CONTENT:");
        hasher.update(expanded_content.as_bytes());
        
        // Hash context variables that affect expansion
        hasher.update(b"@CONTEXT:");
        let mut sorted_context: Vec<_> = context_vars.iter().collect();
        sorted_context.sort_by_key(|(name, _)| *name);
        for (name, value) in sorted_context {
            hasher.update(name.as_bytes());
            hasher.update(b"=");
            hasher.update(value.as_bytes());
            hasher.update(b";");
        }
        
        let result = hasher.finalize();
        let mut bytes = [0u8; 16];
        bytes.copy_from_slice(&result[0..16]);
        u128::from_be_bytes(bytes)
    }
    
    /// Hash TeX counter states with proper dependency tracking
    /// This ensures that changes to counters invalidate dependent cache entries
    pub fn hash_counter_states(
        counters: &HashMap<String, i32>,
        auxiliary_counters: &HashMap<String, i32>,
        counter_dependencies: &HashMap<String, Vec<String>>
    ) -> u128 {
        let mut hasher = SipHasher13::new();
        
        // Hash main counters (page, chapter, section, etc.)
        let mut sorted_counters: Vec<_> = counters.iter().collect();
        sorted_counters.sort_by_key(|(name, _)| *name);
        for (name, value) in sorted_counters {
            name.hash(&mut hasher);
            value.hash(&mut hasher);
        }
        
        // Hash auxiliary counters (footnote, figure, table, etc.)
        let mut sorted_aux: Vec<_> = auxiliary_counters.iter().collect();
        sorted_aux.sort_by_key(|(name, _)| *name);
        for (name, value) in sorted_aux {
            name.hash(&mut hasher);
            value.hash(&mut hasher);
        }
        
        // Hash counter dependency relationships
        let mut sorted_deps: Vec<_> = counter_dependencies.iter().collect();
        sorted_deps.sort_by_key(|(name, _)| *name);
        for (counter_name, deps) in sorted_deps {
            counter_name.hash(&mut hasher);
            let mut sorted_dep_list = deps.clone();
            sorted_dep_list.sort();
            for dep in sorted_dep_list {
                dep.hash(&mut hasher);
            }
        }
        
        hasher.finish128().as_u128()
    }
    
    /// Hash complete TeX state with macro expansion context
    /// This is the comprehensive hash function for full cache key generation
    pub fn hash_complete_tex_state(
        engine_state: &TeXEngineState,
        macro_expansions: &HashMap<String, String>,
        counter_state: &HashMap<String, i32>,
        current_position: usize
    ) -> u128 {
        let mut hasher = SipHasher13::new();
        
        // Hash base engine state
        let base_state_hash = Self::hash_tex_state(engine_state);
        base_state_hash.hash(&mut hasher);
        
        // Hash macro expansion state
        let macro_hash = Self::hash_macro_definitions(macro_expansions);
        macro_hash.hash(&mut hasher);
        
        // Hash counter state
        let counter_hash = Self::hash_content(counter_state);
        counter_hash.hash(&mut hasher);
        
        // Hash current position for context-sensitive caching
        current_position.hash(&mut hasher);
        
        hasher.finish128().as_u128()
    }
    
    /// Compute hash for spatial constraints
    pub fn hash_constraints(constraints: &SpatialConstraints) -> u128 {
        let mut hasher = SipHasher128::new();
        
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
        
        hasher.finish128()
    }
    
    /// Hash paragraph content with formatting parameters
    pub fn hash_paragraph_content(content: &str, par_params: &ParagraphParams) -> u128 {
        let mut hasher = Sha256::new();
        
        // Hash the paragraph content
        hasher.update(content.as_bytes());
        
        // Hash paragraph parameters that affect layout
        hasher.update(&par_params.line_width.to_be_bytes());
        hasher.update(&par_params.left_indent.to_be_bytes());
        hasher.update(&par_params.right_indent.to_be_bytes());
        hasher.update(&par_params.first_line_indent.to_be_bytes());
        hasher.update(&par_params.par_skip.to_be_bytes());
        hasher.update(&par_params.baseline_skip.to_be_bytes());
        hasher.update(&par_params.line_spacing.to_be_bytes());
        
        let result = hasher.finalize();
        let mut bytes = [0u8; 16];
        bytes.copy_from_slice(&result[0..16]);
        u128::from_be_bytes(bytes)
    }
    
    /// Hash math content with display mode flag
    pub fn hash_math_content(content: &str, is_display: bool) -> u128 {
        let mut hasher = Sha256::new();
        
        // Hash the math content
        hasher.update(content.as_bytes());
        
        // Include display mode flag
        hasher.update(&[if is_display { 1 } else { 0 }]);
        
        let result = hasher.finalize();
        let mut bytes = [0u8; 16];
        bytes.copy_from_slice(&result[0..16]);
        u128::from_be_bytes(bytes)
    }
}

/// Macro expansion tracking system for dependency analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MacroTracker {
    /// Map of macro name to its definition and usage count
    pub macro_definitions: HashMap<String, MacroDefinition>,
    /// Stack of macro expansions currently in progress
    pub expansion_stack: Vec<MacroExpansion>,
    /// Hash of all macro states for cache invalidation
    pub macro_state_hash: u128,
    /// Track if any macros have changed since last hash
    pub dirty: bool,
}

/// Individual macro definition with tracking metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MacroDefinition {
    /// The macro name (e.g., "section", "newcommand")
    pub name: String,
    /// The macro definition body
    pub definition: String,
    /// Number of parameters the macro accepts
    pub param_count: u8,
    /// Hash of the definition for change detection
    pub definition_hash: u128,
    /// Usage count for popularity tracking
    pub usage_count: u64,
    /// When this macro was last defined
    pub defined_at: u64,
    /// Dependencies this macro has on other macros
    pub dependencies: Vec<String>,
}

/// Track a single macro expansion operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MacroExpansion {
    /// Name of the macro being expanded
    pub macro_name: String,
    /// Arguments passed to the macro
    pub arguments: Vec<String>,
    /// Position in source where expansion occurred
    pub source_position: usize,
    /// Hash of the expanded content
    pub expansion_hash: u128,
    /// Timestamp when expansion started
    pub started_at: u64,
    /// Dependencies accessed during expansion
    pub accessed_dependencies: Vec<DependencyKey>,
}

impl MacroTracker {
    /// Create a new macro tracker
    pub fn new() -> Self {
        Self {
            macro_definitions: HashMap::new(),
            expansion_stack: Vec::new(),
            macro_state_hash: 0,
            dirty: false,
        }
    }
    
    /// Record a macro definition
    pub fn define_macro(&mut self, name: String, definition: String, param_count: u8) {
        let definition_hash = HashComputation::hash_tex_source(&definition);
        let timestamp = current_timestamp_us();
        
        let dependencies = self.extract_macro_dependencies(&definition);
        
        let macro_def = MacroDefinition {
            name: name.clone(),
            definition,
            param_count,
            definition_hash,
            usage_count: 0,
            defined_at: timestamp,
            dependencies,
        };
        
        // Check if this is a new or changed definition
        if let Some(existing) = self.macro_definitions.get(&name) {
            if existing.definition_hash != definition_hash {
                self.dirty = true;
            }
        } else {
            self.dirty = true;
        }
        
        self.macro_definitions.insert(name, macro_def);
        self.update_state_hash();
    }
    
    /// Start tracking a macro expansion
    pub fn begin_expansion(&mut self, macro_name: String, arguments: Vec<String>, source_position: usize) -> Option<usize> {
        if let Some(macro_def) = self.macro_definitions.get_mut(&macro_name) {
            macro_def.usage_count += 1;
            
            let expansion = MacroExpansion {
                macro_name: macro_name.clone(),
                arguments: arguments.clone(),
                source_position,
                expansion_hash: 0, // Will be set when expansion completes
                started_at: current_timestamp_us(),
                accessed_dependencies: Vec::new(),
            };
            
            self.expansion_stack.push(expansion);
            Some(self.expansion_stack.len() - 1)
        } else {
            None
        }
    }
    
    /// Complete a macro expansion and record its hash
    pub fn end_expansion(&mut self, expansion_id: usize, expanded_content: &str) -> Option<u128> {
        if expansion_id < self.expansion_stack.len() {
            let expansion_hash = HashComputation::hash_tex_source(expanded_content);
            self.expansion_stack[expansion_id].expansion_hash = expansion_hash;
            Some(expansion_hash)
        } else {
            None
        }
    }
    
    /// Record a dependency access during macro expansion
    pub fn record_dependency_access(&mut self, dependency: DependencyKey) {
        if let Some(current_expansion) = self.expansion_stack.last_mut() {
            current_expansion.accessed_dependencies.push(dependency);
        }
    }
    
    /// Check if any tracked macros have changed
    pub fn has_changes(&self) -> bool {
        self.dirty
    }
    
    /// Get the current macro state hash
    pub fn get_state_hash(&self) -> u128 {
        self.macro_state_hash
    }
    
    /// Update the overall macro state hash
    fn update_state_hash(&mut self) {
        let macro_map: HashMap<String, String> = self.macro_definitions
            .iter()
            .map(|(name, def)| (name.clone(), def.definition.clone()))
            .collect();
            
        self.macro_state_hash = HashComputation::hash_macro_definitions(&macro_map);
    }
    
    /// Extract macro dependencies from definition text
    fn extract_macro_dependencies(&self, definition: &str) -> Vec<String> {
        let mut dependencies = Vec::new();
        
        // Simple regex-like parsing for \macro_name patterns
        let mut chars = definition.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == '\\' {
                let mut macro_name = String::new();
                while let Some(&next_ch) = chars.peek() {
                    if next_ch.is_alphabetic() {
                        macro_name.push(chars.next().unwrap());
                    } else {
                        break;
                    }
                }
                
                if !macro_name.is_empty() && self.macro_definitions.contains_key(&macro_name) {
                    dependencies.push(macro_name);
                }
            }
        }
        
        dependencies.sort();
        dependencies.dedup();
        dependencies
    }
    
    /// Get macros that depend on a given macro
    pub fn get_dependent_macros(&self, macro_name: &str) -> Vec<String> {
        self.macro_definitions
            .iter()
            .filter(|(_, def)| def.dependencies.contains(&macro_name.to_string()))
            .map(|(name, _)| name.clone())
            .collect()
    }
    
    /// Invalidate cache entries that depend on changed macros
    pub fn get_invalidated_dependencies(&self, changed_macro: &str) -> Vec<DependencyKey> {
        let mut invalidated = Vec::new();
        
        // Direct dependency
        invalidated.push(DependencyKey {
            dep_type: DependencyType::Macro,
            identifier: changed_macro.to_string(),
            value_hash: self.macro_definitions
                .get(changed_macro)
                .map(|def| def.definition_hash)
                .unwrap_or(0),
        });
        
        // Transitive dependencies
        for dependent in self.get_dependent_macros(changed_macro) {
            invalidated.push(DependencyKey {
                dep_type: DependencyType::Macro,
                identifier: dependent.clone(),
                value_hash: self.macro_definitions
                    .get(&dependent)
                    .map(|def| def.definition_hash)
                    .unwrap_or(0),
            });
        }
        
        invalidated
    }
    
    /// Reset dirty flag after processing changes
    pub fn mark_clean(&mut self) {
        self.dirty = false;
    }
    
    /// Get statistics about macro usage
    pub fn get_usage_statistics(&self) -> MacroStatistics {
        let total_macros = self.macro_definitions.len();
        let total_expansions: u64 = self.macro_definitions
            .values()
            .map(|def| def.usage_count)
            .sum();
            
        let most_used_macro = self.macro_definitions
            .iter()
            .max_by_key(|(_, def)| def.usage_count)
            .map(|(name, def)| (name.clone(), def.usage_count));
            
        MacroStatistics {
            total_macros,
            total_expansions,
            most_used_macro,
            current_expansion_depth: self.expansion_stack.len(),
        }
    }
}

/// Statistics about macro usage for performance monitoring
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MacroStatistics {
    /// Total number of defined macros
    pub total_macros: usize,
    /// Total number of macro expansions performed
    pub total_expansions: u64,
    /// Most frequently used macro and its usage count
    pub most_used_macro: Option<(String, u64)>,
    /// Current depth of nested macro expansions
    pub current_expansion_depth: usize,
}

/// Register tracking system for TeX counters, dimensions, skips, and glue
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterTracker {
    /// Count registers (integer values)
    pub count_registers: HashMap<i32, RegisterValue<i32>>,
    /// Dimension registers (scaled points)
    pub dimen_registers: HashMap<i32, RegisterValue<i32>>,
    /// Skip registers (glue values)
    pub skip_registers: HashMap<i32, RegisterValue<GlueValue>>,
    /// Glue registers (mutable glue)
    pub glue_registers: HashMap<i32, RegisterValue<GlueValue>>,
    /// Snapshot of last known register state
    pub last_snapshot_hash: u128,
    /// Track which registers have changed
    pub dirty_registers: Vec<RegisterChange>,
    /// Change sequence number for ordering
    pub change_sequence: u64,
}

/// Individual register value with change tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterValue<T> {
    /// Current value of the register
    pub value: T,
    /// Hash of the current value
    pub value_hash: u128,
    /// When this register was last modified
    pub last_modified: u64,
    /// How many times this register has been modified
    pub modification_count: u64,
    /// Source location where last modification occurred
    pub last_modified_source: Option<usize>,
}

/// TeX glue value (width + stretch + shrink)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GlueValue {
    /// Fixed width component (scaled points)
    pub width: i32,
    /// Stretch component (scaled points) 
    pub stretch: i32,
    /// Shrink component (scaled points)
    pub shrink: i32,
    /// Stretch order (0=pt, 1=fil, 2=fill, 3=filll)
    pub stretch_order: i8,
    /// Shrink order
    pub shrink_order: i8,
}

/// Record of a register change for dependency tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterChange {
    /// Type of register that changed
    pub register_type: RegisterType,
    /// Register number
    pub register_number: i32,
    /// Hash of the old value
    pub old_value_hash: u128,
    /// Hash of the new value
    pub new_value_hash: u128,
    /// When the change occurred
    pub timestamp: u64,
    /// Source position where change was made
    pub source_position: Option<usize>,
    /// Change sequence number
    pub sequence: u64,
}

/// Types of TeX registers
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Hash)]
pub enum RegisterType {
    /// \count registers
    Count,
    /// \dimen registers  
    Dimen,
    /// \skip registers
    Skip,
    /// \glue registers (mutable)
    Glue,
    /// \toks registers
    Toks,
}

/// Result of a memoization cache lookup with detailed match information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoLookupResult {
    /// Hash of the cached result
    pub result_hash: u128,
    /// Type of cache match that was found
    pub match_type: CacheMatchType,
    /// The actual constraints that are satisfied by this cache entry
    pub constraints_satisfied: SpatialConstraints,
    /// Dependencies that were accessed during the original computation
    pub dependencies: Vec<DependencyKey>,
    /// Metadata about the cached computation
    pub metadata: CacheMetadata,
}

/// Types of cache matches that can occur during lookup
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CacheMatchType {
    /// Exact match on content, constraints, and state
    Exact,
    /// Content and state match, constraints are compatible via subsumption
    Subsumption,
    /// Content and state match, constraints are compatible within tolerance
    Compatible,
    /// Different content but same dependencies (rare, for whitespace/comment changes)
    DependencyBased,
}

/// Information about constraint compatibility between required and cached constraints
#[derive(Debug, Clone)]
pub struct ConstraintCompatibility {
    /// Type of compatibility match
    pub match_type: CacheMatchType,
    /// The effective constraints that result from this match
    pub effective_constraints: SpatialConstraints,
    /// Compatibility score from 0.0 (incompatible) to 1.0 (identical)
    pub compatibility_score: f64,
}

impl RegisterTracker {
    /// Create a new register tracker
    pub fn new() -> Self {
        Self {
            count_registers: HashMap::new(),
            dimen_registers: HashMap::new(),
            skip_registers: HashMap::new(),
            glue_registers: HashMap::new(),
            last_snapshot_hash: 0,
            dirty_registers: Vec::new(),
            change_sequence: 0,
        }
    }
    
    /// Set a count register value
    pub fn set_count(&mut self, register: i32, value: i32, source_position: Option<usize>) {
        let value_hash = HashComputation::hash_content(&value);
        let timestamp = current_timestamp_us();
        
        if let Some(existing) = self.count_registers.get(&register) {
            if existing.value_hash != value_hash {
                self.record_register_change(RegisterType::Count, register, existing.value_hash, value_hash, source_position);
            }
        } else {
            self.record_register_change(RegisterType::Count, register, 0, value_hash, source_position);
        }
        
        self.count_registers.insert(register, RegisterValue {
            value,
            value_hash,
            last_modified: timestamp,
            modification_count: self.count_registers.get(&register).map(|r| r.modification_count + 1).unwrap_or(1),
            last_modified_source: source_position,
        });
    }
    
    /// Set a dimension register value
    pub fn set_dimen(&mut self, register: i32, value: i32, source_position: Option<usize>) {
        let value_hash = HashComputation::hash_content(&value);
        let timestamp = current_timestamp_us();
        
        if let Some(existing) = self.dimen_registers.get(&register) {
            if existing.value_hash != value_hash {
                self.record_register_change(RegisterType::Dimen, register, existing.value_hash, value_hash, source_position);
            }
        } else {
            self.record_register_change(RegisterType::Dimen, register, 0, value_hash, source_position);
        }
        
        self.dimen_registers.insert(register, RegisterValue {
            value,
            value_hash,
            last_modified: timestamp,
            modification_count: self.dimen_registers.get(&register).map(|r| r.modification_count + 1).unwrap_or(1),
            last_modified_source: source_position,
        });
    }
    
    /// Set a skip register value
    pub fn set_skip(&mut self, register: i32, value: GlueValue, source_position: Option<usize>) {
        let value_hash = HashComputation::hash_content(&value);
        let timestamp = current_timestamp_us();
        
        if let Some(existing) = self.skip_registers.get(&register) {
            if existing.value_hash != value_hash {
                self.record_register_change(RegisterType::Skip, register, existing.value_hash, value_hash, source_position);
            }
        } else {
            self.record_register_change(RegisterType::Skip, register, 0, value_hash, source_position);
        }
        
        self.skip_registers.insert(register, RegisterValue {
            value,
            value_hash,
            last_modified: timestamp,
            modification_count: self.skip_registers.get(&register).map(|r| r.modification_count + 1).unwrap_or(1),
            last_modified_source: source_position,
        });
    }
    
    /// Record a register change for dependency tracking
    fn record_register_change(
        &mut self,
        register_type: RegisterType,
        register_number: i32,
        old_value_hash: u128,
        new_value_hash: u128,
        source_position: Option<usize>
    ) {
        self.change_sequence += 1;
        
        let change = RegisterChange {
            register_type,
            register_number,
            old_value_hash,
            new_value_hash,
            timestamp: current_timestamp_us(),
            source_position,
            sequence: self.change_sequence,
        };
        
        self.dirty_registers.push(change);
    }
    
    /// Get all register changes since last snapshot
    pub fn get_changes_since_snapshot(&self) -> &[RegisterChange] {
        &self.dirty_registers
    }
    
    /// Create a snapshot of current register state
    pub fn create_snapshot(&mut self) -> u128 {
        let mut hasher = SipHasher13::new();
        
        // Hash all count registers
        for (&num, value) in &self.count_registers {
            num.hash(&mut hasher);
            value.value_hash.hash(&mut hasher);
        }
        
        // Hash all dimension registers
        for (&num, value) in &self.dimen_registers {
            num.hash(&mut hasher);
            value.value_hash.hash(&mut hasher);
        }
        
        // Hash all skip registers
        for (&num, value) in &self.skip_registers {
            num.hash(&mut hasher);
            value.value_hash.hash(&mut hasher);
        }
        
        // Hash all glue registers
        for (&num, value) in &self.glue_registers {
            num.hash(&mut hasher);
            value.value_hash.hash(&mut hasher);
        }
        
        let snapshot_hash = hasher.finish128().as_u128();
        self.last_snapshot_hash = snapshot_hash;
        self.dirty_registers.clear();
        
        snapshot_hash
    }
    
    /// Check if any registers have changed since last snapshot
    pub fn has_changes(&self) -> bool {
        !self.dirty_registers.is_empty()
    }
    
    /// Get dependencies for all changed registers
    pub fn get_register_dependencies(&self) -> Vec<DependencyKey> {
        let mut dependencies = Vec::new();
        
        for change in &self.dirty_registers {
            dependencies.push(DependencyKey {
                dep_type: DependencyType::Register,
                identifier: format!("{:?}[{}]", change.register_type, change.register_number),
                value_hash: change.new_value_hash,
            });
        }
        
        dependencies
    }
    
    /// Get register statistics for monitoring
    pub fn get_statistics(&self) -> RegisterStatistics {
        RegisterStatistics {
            total_count_registers: self.count_registers.len(),
            total_dimen_registers: self.dimen_registers.len(),
            total_skip_registers: self.skip_registers.len(),
            total_glue_registers: self.glue_registers.len(),
            total_changes: self.change_sequence,
            pending_changes: self.dirty_registers.len(),
            most_modified_register: self.find_most_modified_register(),
        }
    }
    
    /// Find the most frequently modified register
    fn find_most_modified_register(&self) -> Option<(RegisterType, i32, u64)> {
        let mut max_modifications = 0;
        let mut result = None;
        
        for (&num, value) in &self.count_registers {
            if value.modification_count > max_modifications {
                max_modifications = value.modification_count;
                result = Some((RegisterType::Count, num, value.modification_count));
            }
        }
        
        for (&num, value) in &self.dimen_registers {
            if value.modification_count > max_modifications {
                max_modifications = value.modification_count;
                result = Some((RegisterType::Dimen, num, value.modification_count));
            }
        }
        
        for (&num, value) in &self.skip_registers {
            if value.modification_count > max_modifications {
                max_modifications = value.modification_count;
                result = Some((RegisterType::Skip, num, value.modification_count));
            }
        }
        
        result
    }
}

/// Statistics about register usage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterStatistics {
    pub total_count_registers: usize,
    pub total_dimen_registers: usize,
    pub total_skip_registers: usize,
    pub total_glue_registers: usize,
    pub total_changes: u64,
    pub pending_changes: usize,
    pub most_modified_register: Option<(RegisterType, i32, u64)>,
}

impl Hash for GlueValue {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.width.hash(state);
        self.stretch.hash(state);
        self.shrink.hash(state);
        self.stretch_order.hash(state);
        self.shrink_order.hash(state);
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
    fn test_tex_source_hashing() {
        let source1 = r"\documentclass{article}\begin{document}Hello World\end{document}";
        let source2 = r"\documentclass{article}\begin{document}Hello World\end{document}";
        let source3 = r"\documentclass{article}\begin{document}Hello Mars\end{document}";
        
        let hash1 = HashComputation::hash_tex_source(source1);
        let hash2 = HashComputation::hash_tex_source(source2);
        let hash3 = HashComputation::hash_tex_source(source3);
        
        assert_eq!(hash1, hash2);
        assert_ne!(hash1, hash3);
    }
    
    #[test]
    fn test_macro_definition_hashing() {
        let mut macros1 = HashMap::new();
        macros1.insert("newcommand".to_string(), r"\def\hello{world}".to_string());
        macros1.insert("title".to_string(), r"My Document".to_string());
        
        let mut macros2 = HashMap::new();
        macros2.insert("title".to_string(), r"My Document".to_string());
        macros2.insert("newcommand".to_string(), r"\def\hello{world}".to_string());
        
        let mut macros3 = HashMap::new();
        macros3.insert("newcommand".to_string(), r"\def\hello{universe}".to_string());
        macros3.insert("title".to_string(), r"My Document".to_string());
        
        let hash1 = HashComputation::hash_macro_definitions(&macros1);
        let hash2 = HashComputation::hash_macro_definitions(&macros2);
        let hash3 = HashComputation::hash_macro_definitions(&macros3);
        
        // Should be same regardless of insertion order
        assert_eq!(hash1, hash2);
        // Should be different with different content
        assert_ne!(hash1, hash3);
    }
    
    #[test]
    fn test_register_state_hashing() {
        let mut int_params1 = HashMap::new();
        int_params1.insert("page".to_string(), 1);
        int_params1.insert("chapter".to_string(), 5);
        
        let mut dimen_params1 = HashMap::new();
        dimen_params1.insert("textwidth".to_string(), 345000);
        dimen_params1.insert("textheight".to_string(), 550000);
        
        let mut int_params2 = HashMap::new();
        int_params2.insert("chapter".to_string(), 5);
        int_params2.insert("page".to_string(), 1);
        
        let mut dimen_params2 = HashMap::new();
        dimen_params2.insert("textheight".to_string(), 550000);
        dimen_params2.insert("textwidth".to_string(), 345000);
        
        let hash1 = HashComputation::hash_register_state(&int_params1, &dimen_params1);
        let hash2 = HashComputation::hash_register_state(&int_params2, &dimen_params2);
        
        assert_eq!(hash1, hash2);
    }
    
    #[test]
    fn test_incremental_hash_updating() {
        let base_hash = 12345u128;
        let component = "font_size";
        let new_value = 67890u128;
        
        let hash1 = HashComputation::update_incremental_hash(base_hash, component, new_value);
        let hash2 = HashComputation::update_incremental_hash(base_hash, component, new_value);
        let hash3 = HashComputation::update_incremental_hash(base_hash, component, 99999u128);
        
        assert_eq!(hash1, hash2);
        assert_ne!(hash1, hash3);
    }
    
    #[test]
    fn test_file_fingerprint_hashing() {
        let file_path = "/path/to/document.tex";
        let content = b"Hello World";
        let modified_time = 1609459200; // 2021-01-01 00:00:00 UTC
        
        let hash1 = HashComputation::hash_file_fingerprint(file_path, content, modified_time);
        let hash2 = HashComputation::hash_file_fingerprint(file_path, content, modified_time);
        let hash3 = HashComputation::hash_file_fingerprint(file_path, content, modified_time + 1);
        
        assert_eq!(hash1, hash2);
        assert_ne!(hash1, hash3);
    }
    
    #[test]
    fn test_package_state_hashing() {
        let packages1 = vec!["amsmath".to_string(), "graphicx".to_string(), "hyperref".to_string()];
        let mut versions1 = HashMap::new();
        versions1.insert("amsmath".to_string(), "2.17e".to_string());
        versions1.insert("graphicx".to_string(), "1.2a".to_string());
        
        let packages2 = vec!["hyperref".to_string(), "amsmath".to_string(), "graphicx".to_string()];
        let mut versions2 = HashMap::new();
        versions2.insert("graphicx".to_string(), "1.2a".to_string());
        versions2.insert("amsmath".to_string(), "2.17e".to_string());
        
        let hash1 = HashComputation::hash_package_state(&packages1, &versions1);
        let hash2 = HashComputation::hash_package_state(&packages2, &versions2);
        
        // Should be same regardless of order
        assert_eq!(hash1, hash2);
    }
    
    #[test]
    fn test_macro_tracker_definition() {
        let mut tracker = MacroTracker::new();
        
        // Define a simple macro
        tracker.define_macro(
            "hello".to_string(),
            r"\textbf{Hello World}".to_string(),
            0
        );
        
        assert_eq!(tracker.macro_definitions.len(), 1);
        assert!(tracker.has_changes());
        
        let macro_def = tracker.macro_definitions.get("hello").unwrap();
        assert_eq!(macro_def.name, "hello");
        assert_eq!(macro_def.param_count, 0);
        assert_eq!(macro_def.usage_count, 0);
    }
    
    #[test]
    fn test_macro_tracker_expansion() {
        let mut tracker = MacroTracker::new();
        
        // Define a macro
        tracker.define_macro(
            "greeting".to_string(),
            r"\textbf{#1}".to_string(),
            1
        );
        
        // Begin expansion
        let expansion_id = tracker.begin_expansion(
            "greeting".to_string(),
            vec!["Hello".to_string()],
            100
        );
        
        assert!(expansion_id.is_some());
        assert_eq!(tracker.expansion_stack.len(), 1);
        
        // End expansion
        let expansion_hash = tracker.end_expansion(expansion_id.unwrap(), r"\textbf{Hello}");
        assert!(expansion_hash.is_some());
        
        // Check usage count
        let macro_def = tracker.macro_definitions.get("greeting").unwrap();
        assert_eq!(macro_def.usage_count, 1);
    }
    
    #[test]
    fn test_macro_dependency_tracking() {
        let mut tracker = MacroTracker::new();
        
        // Define base macro
        tracker.define_macro(
            "base".to_string(),
            r"\textbf{Base}".to_string(),
            0
        );
        
        // Define dependent macro
        tracker.define_macro(
            "dependent".to_string(),
            r"\base with extra".to_string(),
            0
        );
        
        let dependent_macros = tracker.get_dependent_macros("base");
        assert_eq!(dependent_macros.len(), 1);
        assert!(dependent_macros.contains(&"dependent".to_string()));
    }
    
    #[test]
    fn test_macro_invalidation() {
        let mut tracker = MacroTracker::new();
        
        tracker.define_macro(
            "title".to_string(),
            r"\Large\textbf{#1}".to_string(),
            1
        );
        
        let invalidated = tracker.get_invalidated_dependencies("title");
        assert_eq!(invalidated.len(), 1);
        assert_eq!(invalidated[0].dep_type, DependencyType::Macro);
        assert_eq!(invalidated[0].identifier, "title");
    }
    
    #[test]
    fn test_macro_statistics() {
        let mut tracker = MacroTracker::new();
        
        // Define multiple macros
        tracker.define_macro("macro1".to_string(), r"\textbf{1}".to_string(), 0);
        tracker.define_macro("macro2".to_string(), r"\textit{2}".to_string(), 0);
        tracker.define_macro("macro3".to_string(), r"\underline{3}".to_string(), 0);
        
        // Use macro2 multiple times
        tracker.begin_expansion("macro2".to_string(), vec![], 0);
        tracker.begin_expansion("macro2".to_string(), vec![], 50);
        tracker.begin_expansion("macro1".to_string(), vec![], 100);
        
        let stats = tracker.get_usage_statistics();
        assert_eq!(stats.total_macros, 3);
        assert_eq!(stats.total_expansions, 3);
        assert_eq!(stats.current_expansion_depth, 3);
        
        // macro2 should be most used
        let (most_used_name, usage_count) = stats.most_used_macro.unwrap();
        assert_eq!(most_used_name, "macro2");
        assert_eq!(usage_count, 2);
    }
    
    #[test]
    fn test_macro_state_hash_changes() {
        let mut tracker = MacroTracker::new();
        
        let initial_hash = tracker.get_state_hash();
        
        // Define a macro
        tracker.define_macro("test".to_string(), r"\textbf{Test}".to_string(), 0);
        let hash_after_define = tracker.get_state_hash();
        
        assert_ne!(initial_hash, hash_after_define);
        assert!(tracker.has_changes());
        
        // Mark clean
        tracker.mark_clean();
        assert!(!tracker.has_changes());
        
        // Redefine with different content
        tracker.define_macro("test".to_string(), r"\textit{Test}".to_string(), 0);
        let hash_after_redefine = tracker.get_state_hash();
        
        assert_ne!(hash_after_define, hash_after_redefine);
        assert!(tracker.has_changes());
    }
    
    #[test]
    fn test_font_state_hashing() {
        // Create mock font state
        let font_state = crate::wasm::FontState {
            cur_f: 12,
            cur_c: 65,  // 'A'
            cur_size: 12000, // 12pt in scaled points
            char_base: vec![100, 200, 300, 400],
            width_base: vec![1000, 2000, 3000],
            height_base: vec![800, 900, 1000],
            depth_base: vec![200, 250, 300],
            italic_base: vec![50, 75, 100],
            param_base: vec![1000, 500, 250],
            font_mem_size: 10000,
            font_max: 255,
            fmem_ptr: 5000,
        };
        
        let hash1 = HashComputation::hash_font_state(&font_state);
        let hash2 = HashComputation::hash_font_state(&font_state);
        
        // Same font state should produce same hash
        assert_eq!(hash1, hash2);
        
        // Different font state should produce different hash
        let mut font_state2 = font_state.clone();
        font_state2.cur_f = 13; // Different font
        let hash3 = HashComputation::hash_font_state(&font_state2);
        assert_ne!(hash1, hash3);
    }
    
    #[test]
    fn test_font_metrics_with_constraints() {
        let font_state = crate::wasm::FontState {
            cur_f: 10,
            cur_c: 97,  // 'a'
            cur_size: 10000, // 10pt
            char_base: vec![1, 2, 3],
            width_base: vec![1000],
            height_base: vec![800],
            depth_base: vec![200],
            italic_base: vec![50],
            param_base: vec![1000],
            font_mem_size: 8000,
            font_max: 100,
            fmem_ptr: 4000,
        };
        
        let constraints = SpatialConstraints {
            min_width: Some(100.0),
            max_width: Some(400.0),
            available_width: Some(300.0),
            natural_width: Some(250.0),
            line_spacing: Some(1.2),
            font_metrics: Some(FontMetrics {
                size: 10.0,
                family: "Computer Modern".to_string(),
                char_width: Some(5.5),
                line_height: 1.2,
                x_height: 4.3,
                baseline_skip: 12.0,
            }),
            min_height: None,
            max_height: None,
            available_height: None,
            stretch: None,
            shrink: None,
            paragraph_shape: None,
            math_spacing: None,
            language_settings: None,
        };
        
        let hash1 = HashComputation::hash_font_metrics_with_constraints(&font_state, &constraints);
        let hash2 = HashComputation::hash_font_metrics_with_constraints(&font_state, &constraints);
        
        assert_eq!(hash1, hash2);
        
        // Change font size in constraints
        let mut constraints2 = constraints.clone();
        if let Some(ref mut font_metrics) = constraints2.font_metrics {
            font_metrics.size = 12.0;
        }
        let hash3 = HashComputation::hash_font_metrics_with_constraints(&font_state, &constraints2);
        assert_ne!(hash1, hash3);
    }
    
    #[test]
    fn test_font_dependency_creation() {
        let font_name = "Times New Roman";
        let font_size = 12.0;
        let font_metrics_hash = 0x123456789ABCDEF0u128;
        
        let dep_key = HashComputation::create_font_dependency(font_name, font_size, font_metrics_hash);
        
        assert_eq!(dep_key.dep_type, DependencyType::Font);
        assert_eq!(dep_key.identifier, "Times New Roman@12.0pt");
        assert_eq!(dep_key.value_hash, font_metrics_hash);
    }
    
    #[test]
    fn test_i32_array_checksum() {
        let array1 = vec![1, 2, 3, 4, 5];
        let array2 = vec![1, 2, 3, 4, 5];
        let array3 = vec![1, 2, 3, 4, 6];
        let array4 = vec![1, 2, 3, 4, 5, 6]; // Different length
        
        let checksum1 = HashComputation::hash_i32_array_checksum(&array1);
        let checksum2 = HashComputation::hash_i32_array_checksum(&array2);
        let checksum3 = HashComputation::hash_i32_array_checksum(&array3);
        let checksum4 = HashComputation::hash_i32_array_checksum(&array4);
        
        assert_eq!(checksum1, checksum2);
        assert_ne!(checksum1, checksum3);
        assert_ne!(checksum1, checksum4);
    }
    
    #[test]
    fn test_register_tracker_count_registers() {
        let mut tracker = RegisterTracker::new();
        
        // Set some count registers
        tracker.set_count(0, 1, Some(100));  // \count0 = 1
        tracker.set_count(255, 42, Some(200)); // \count255 = 42
        
        assert_eq!(tracker.count_registers.len(), 2);
        assert!(tracker.has_changes());
        
        let count0 = tracker.count_registers.get(&0).unwrap();
        assert_eq!(count0.value, 1);
        assert_eq!(count0.modification_count, 1);
        assert_eq!(count0.last_modified_source, Some(100));
        
        // Change count0 again
        tracker.set_count(0, 5, Some(300));
        let count0_updated = tracker.count_registers.get(&0).unwrap();
        assert_eq!(count0_updated.value, 5);
        assert_eq!(count0_updated.modification_count, 2);
        
        // Should have 3 changes recorded (initial 0, initial 255, update 0)
        let changes = tracker.get_changes_since_snapshot();
        assert_eq!(changes.len(), 3);
    }
    
    #[test]
    fn test_register_tracker_dimen_registers() {
        let mut tracker = RegisterTracker::new();
        
        // Set dimension registers (scaled points)
        tracker.set_dimen(1, 65536, Some(50));    // \dimen1 = 1pt (65536 sp)
        tracker.set_dimen(2, 720896, Some(75));   // \dimen2 = 11pt
        
        assert_eq!(tracker.dimen_registers.len(), 2);
        
        let dimen1 = tracker.dimen_registers.get(&1).unwrap();
        assert_eq!(dimen1.value, 65536);
        
        // Create snapshot
        let snapshot_hash = tracker.create_snapshot();
        assert!(!tracker.has_changes());
        
        // Change a register after snapshot
        tracker.set_dimen(1, 131072, Some(100)); // 2pt
        assert!(tracker.has_changes());
        
        let new_snapshot = tracker.create_snapshot();
        assert_ne!(snapshot_hash, new_snapshot);
    }
    
    #[test]
    fn test_register_tracker_skip_registers() {
        let mut tracker = RegisterTracker::new();
        
        let glue_value = GlueValue {
            width: 65536,      // 1pt
            stretch: 32768,    // 0.5pt
            shrink: 16384,     // 0.25pt
            stretch_order: 0,  // normal (pt)
            shrink_order: 0,   // normal (pt)
        };
        
        tracker.set_skip(0, glue_value.clone(), Some(200));
        
        assert_eq!(tracker.skip_registers.len(), 1);
        let skip0 = tracker.skip_registers.get(&0).unwrap();
        assert_eq!(skip0.value, glue_value);
    }
    
    #[test]
    fn test_register_dependencies() {
        let mut tracker = RegisterTracker::new();
        
        tracker.set_count(0, 1, None);
        tracker.set_dimen(1, 65536, None);
        tracker.set_count(0, 2, None); // Change count0 again
        
        let dependencies = tracker.get_register_dependencies();
        assert_eq!(dependencies.len(), 3);
        
        // Check that we have the right dependency types
        let count_deps: Vec<_> = dependencies.iter()
            .filter(|dep| dep.dep_type == DependencyType::Register && dep.identifier.starts_with("Count"))
            .collect();
        assert_eq!(count_deps.len(), 2); // Two changes to count registers
        
        let dimen_deps: Vec<_> = dependencies.iter()
            .filter(|dep| dep.dep_type == DependencyType::Register && dep.identifier.starts_with("Dimen"))
            .collect();
        assert_eq!(dimen_deps.len(), 1);
    }
    
    #[test]
    fn test_register_statistics() {
        let mut tracker = RegisterTracker::new();
        
        // Add various registers
        tracker.set_count(0, 1, None);
        tracker.set_count(1, 2, None);
        tracker.set_dimen(0, 65536, None);
        tracker.set_skip(0, GlueValue {
            width: 0, stretch: 65536, shrink: 0,
            stretch_order: 1, shrink_order: 0
        }, None);
        
        // Modify count0 multiple times to make it most modified
        tracker.set_count(0, 2, None);
        tracker.set_count(0, 3, None);
        tracker.set_count(0, 4, None);
        
        let stats = tracker.get_statistics();
        assert_eq!(stats.total_count_registers, 2);
        assert_eq!(stats.total_dimen_registers, 1);
        assert_eq!(stats.total_skip_registers, 1);
        assert_eq!(stats.total_glue_registers, 0);
        assert_eq!(stats.total_changes, 7); // 4 + 1 + 1 + 1 = 7 changes
        
        // count0 should be most modified (4 times)
        let (reg_type, reg_num, mod_count) = stats.most_modified_register.unwrap();
        assert_eq!(reg_type, RegisterType::Count);
        assert_eq!(reg_num, 0);
        assert_eq!(mod_count, 4);
    }
    
    #[test]
    fn test_glue_value_hashing() {
        let glue1 = GlueValue {
            width: 65536,
            stretch: 32768,
            shrink: 16384,
            stretch_order: 0,
            shrink_order: 0,
        };
        
        let glue2 = GlueValue {
            width: 65536,
            stretch: 32768,
            shrink: 16384,
            stretch_order: 0,
            shrink_order: 0,
        };
        
        let glue3 = GlueValue {
            width: 65536,
            stretch: 32768,
            shrink: 16384,
            stretch_order: 1,  // Different stretch order
            shrink_order: 0,
        };
        
        let hash1 = HashComputation::hash_content(&glue1);
        let hash2 = HashComputation::hash_content(&glue2);
        let hash3 = HashComputation::hash_content(&glue3);
        
        assert_eq!(hash1, hash2);
        assert_ne!(hash1, hash3);
    }
    
    #[test]
    fn test_memoization_system_with_registers() {
        let mut memo_system = MemoizationSystem::new(1000, 100);
        
        // Set some registers
        memo_system.set_count_register(0, 1, Some(100));
        memo_system.set_dimen_register(1, 65536, Some(200));
        
        assert!(memo_system.has_register_changes());
        
        // Get comprehensive statistics
        let (cache_stats, macro_stats, register_stats) = memo_system.get_comprehensive_statistics();
        assert_eq!(register_stats.total_count_registers, 1);
        assert_eq!(register_stats.total_dimen_registers, 1);
        assert_eq!(register_stats.total_changes, 2);
        
        // Get register state hash
        let state_hash = memo_system.get_register_state_hash();
        assert!(!memo_system.has_register_changes()); // Should be clean after snapshot
        
        // Change a register and verify hash changes
        memo_system.set_count_register(0, 2, Some(300));
        let new_state_hash = memo_system.get_register_state_hash();
        assert_ne!(state_hash, new_state_hash);
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
    
    #[test]
    fn test_check_memo_exact_match() {
        let mut memo_system = MemoizationSystem::new(1000, 100);
        
        // Create test constraints
        let constraints = SpatialConstraints {
            min_width: Some(100.0),
            max_width: Some(200.0),
            available_width: Some(150.0),
            natural_width: Some(120.0),
            min_height: None,
            max_height: None,
            available_height: None,
            stretch: Some(10.0),
            shrink: Some(5.0),
            line_spacing: Some(1.2),
            paragraph_shape: None,
            math_spacing: None,
            font_metrics: None,
            language_settings: None,
        };
        
        let content_hash = HashComputation::hash_tex_source("\\textbf{Hello World}");
        let state_hash = 12345u128;
        let constraint_hash = HashComputation::hash_constraints(&constraints);
        
        // Create and store a cache entry
        let cache_key = CacheKey {
            content_hash,
            constraint_hash,
            state_hash,
        };
        
        let cache_entry = CacheEntry {
            result_hash: 98765u128,
            constraints: constraints.clone(),
            dependencies: vec![],
            metadata: CacheMetadata {
                source_position: 100,
                context_type: "paragraph".to_string(),
                result_size_bytes: 1024,
                metrics: EntryMetrics {
                    computation_time_us: 5000,
                    memory_used_bytes: 2048,
                    tex_operations: 10,
                    hit_rate: 1.0,
                },
            },
            timestamp: current_timestamp_us(),
            access_count: 0,
            last_access: 0,
        };
        
        memo_system.cache.insert(cache_key, cache_entry);
        
        // Test exact match lookup
        let result = memo_system.check_memo(content_hash, &constraints, state_hash, 100);
        
        assert!(result.is_some());
        let memo_result = result.unwrap();
        assert_eq!(memo_result.result_hash, 98765u128);
        assert_eq!(memo_result.match_type, CacheMatchType::Exact);
        assert_eq!(memo_result.constraints_satisfied, constraints);
        
        // Verify cache hit was recorded
        assert_eq!(memo_system.stats.cache_hits, 1);
        assert_eq!(memo_system.stats.total_lookups, 1);
    }
    
    #[test]
    fn test_check_memo_constraint_subsumption() {
        let mut memo_system = MemoizationSystem::new(1000, 100);
        
        // Create cached constraints (more general/less restrictive)
        let cached_constraints = SpatialConstraints {
            min_width: Some(50.0),   // Less restrictive
            max_width: Some(300.0),  // Less restrictive
            available_width: Some(200.0),
            natural_width: Some(120.0),
            min_height: None,
            max_height: None,
            available_height: None,
            stretch: Some(10.0),
            shrink: Some(5.0),
            line_spacing: Some(1.2),
            paragraph_shape: None,
            math_spacing: None,
            font_metrics: None,
            language_settings: None,
        };
        
        // Create required constraints (more specific/restrictive)
        let required_constraints = SpatialConstraints {
            min_width: Some(100.0),  // More restrictive
            max_width: Some(200.0),  // More restrictive
            available_width: Some(150.0),
            natural_width: Some(120.0),
            min_height: None,
            max_height: None,
            available_height: None,
            stretch: Some(10.0),
            shrink: Some(5.0),
            line_spacing: Some(1.2),
            paragraph_shape: None,
            math_spacing: None,
            font_metrics: None,
            language_settings: None,
        };
        
        let content_hash = HashComputation::hash_tex_source("\\section{Test}");
        let state_hash = 54321u128;
        
        // Store cache entry with less restrictive constraints
        let cache_key = CacheKey {
            content_hash,
            constraint_hash: HashComputation::hash_constraints(&cached_constraints),
            state_hash,
        };
        
        let cache_entry = CacheEntry {
            result_hash: 11111u128,
            constraints: cached_constraints.clone(),
            dependencies: vec![],
            metadata: CacheMetadata {
                source_position: 200,
                context_type: "section".to_string(),
                result_size_bytes: 512,
                metrics: EntryMetrics {
                    computation_time_us: 3000,
                    memory_used_bytes: 1024,
                    tex_operations: 5,
                    hit_rate: 0.8,
                },
            },
            timestamp: current_timestamp_us(),
            access_count: 0,
            last_access: 0,
        };
        
        memo_system.cache.insert(cache_key.clone(), cache_entry);
        
        // Add to spatial index for constraint-based lookup
        memo_system.spatial_index.insert(cache_key, &cached_constraints);
        
        // Test subsumption-based lookup
        let result = memo_system.check_memo(content_hash, &required_constraints, state_hash, 200);
        
        assert!(result.is_some());
        let memo_result = result.unwrap();
        assert_eq!(memo_result.result_hash, 11111u128);
        assert_eq!(memo_result.match_type, CacheMatchType::Subsumption);
        
        // Verify cache hit was recorded
        assert_eq!(memo_system.stats.cache_hits, 1);
        assert_eq!(memo_system.stats.total_lookups, 1);
    }
    
    #[test]
    fn test_check_memo_cache_miss() {
        let mut memo_system = MemoizationSystem::new(1000, 100);
        
        let constraints = SpatialConstraints {
            min_width: Some(100.0),
            max_width: Some(200.0),
            available_width: Some(150.0),
            natural_width: Some(120.0),
            min_height: None,
            max_height: None,
            available_height: None,
            stretch: Some(10.0),
            shrink: Some(5.0),
            line_spacing: Some(1.2),
            paragraph_shape: None,
            math_spacing: None,
            font_metrics: None,
            language_settings: None,
        };
        
        let content_hash = HashComputation::hash_tex_source("\\textit{Not cached}");
        let state_hash = 99999u128;
        
        // Test cache miss
        let result = memo_system.check_memo(content_hash, &constraints, state_hash, 300);
        
        assert!(result.is_none());
        
        // Verify cache miss was recorded
        assert_eq!(memo_system.stats.cache_misses, 1);
        assert_eq!(memo_system.stats.total_lookups, 1);
        assert_eq!(memo_system.stats.hit_rate(), 0.0);
    }
    
    #[test]
    fn test_check_memo_compatible_constraints() {
        let mut memo_system = MemoizationSystem::new(1000, 100);
        
        // Create constraints that are compatible but not identical
        let cached_constraints = SpatialConstraints {
            min_width: Some(100.1),  // Within tolerance
            max_width: Some(199.9),  // Within tolerance
            available_width: Some(150.05),
            natural_width: Some(119.95),
            min_height: None,
            max_height: None,
            available_height: None,
            stretch: Some(10.02),
            shrink: Some(4.98),
            line_spacing: Some(1.201),
            paragraph_shape: None,
            math_spacing: None,
            font_metrics: None,
            language_settings: None,
        };
        
        let required_constraints = SpatialConstraints {
            min_width: Some(100.0),
            max_width: Some(200.0),
            available_width: Some(150.0),
            natural_width: Some(120.0),
            min_height: None,
            max_height: None,
            available_height: None,
            stretch: Some(10.0),
            shrink: Some(5.0),
            line_spacing: Some(1.2),
            paragraph_shape: None,
            math_spacing: None,
            font_metrics: None,
            language_settings: None,
        };
        
        let content_hash = HashComputation::hash_tex_source("\\emph{Compatible}");
        let state_hash = 77777u128;
        
        // Store cache entry
        let cache_key = CacheKey {
            content_hash,
            constraint_hash: HashComputation::hash_constraints(&cached_constraints),
            state_hash,
        };
        
        let cache_entry = CacheEntry {
            result_hash: 22222u128,
            constraints: cached_constraints.clone(),
            dependencies: vec![],
            metadata: CacheMetadata {
                source_position: 400,
                context_type: "emphasis".to_string(),
                result_size_bytes: 256,
                metrics: EntryMetrics {
                    computation_time_us: 2000,
                    memory_used_bytes: 512,
                    tex_operations: 3,
                    hit_rate: 0.9,
                },
            },
            timestamp: current_timestamp_us(),
            access_count: 0,
            last_access: 0,
        };
        
        memo_system.cache.insert(cache_key.clone(), cache_entry);
        memo_system.spatial_index.insert(cache_key, &cached_constraints);
        
        // Test compatible constraint lookup
        let result = memo_system.check_memo(content_hash, &required_constraints, state_hash, 400);
        
        assert!(result.is_some());
        let memo_result = result.unwrap();
        assert_eq!(memo_result.result_hash, 22222u128);
        assert_eq!(memo_result.match_type, CacheMatchType::Compatible);
        
        // Verify cache hit was recorded
        assert_eq!(memo_system.stats.cache_hits, 1);
        assert_eq!(memo_system.stats.total_lookups, 1);
    }
    
    #[test]
    fn test_constraint_compatibility_calculation() {
        let memo_system = MemoizationSystem::new(1000, 100);
        
        let required = SpatialConstraints {
            min_width: Some(100.0),
            max_width: Some(200.0),
            available_width: Some(150.0),
            natural_width: Some(120.0),
            min_height: Some(50.0),
            max_height: Some(100.0),
            available_height: Some(75.0),
            stretch: Some(10.0),
            shrink: Some(5.0),
            line_spacing: Some(1.2),
            paragraph_shape: None,
            math_spacing: None,
            font_metrics: None,
            language_settings: None,
        };
        
        let cached = SpatialConstraints {
            min_width: Some(105.0),   // 5% difference
            max_width: Some(190.0),   // 5% difference
            available_width: Some(155.0), // ~3% difference
            natural_width: Some(125.0),   // ~4% difference
            min_height: Some(48.0),   // 4% difference
            max_height: Some(95.0),   // 5% difference
            available_height: Some(72.0), // 4% difference
            stretch: Some(9.5),       // 5% difference
            shrink: Some(5.2),        // 4% difference
            line_spacing: Some(1.18), // ~2% difference
            paragraph_shape: None,
            math_spacing: None,
            font_metrics: None,
            language_settings: None,
        };
        
        let score = memo_system.calculate_compatibility_score(&required, &cached);
        
        // Score should be high (close to 1.0) since differences are small
        assert!(score > 0.9);
        assert!(score < 1.0);
    }
    
    #[test]
    fn test_check_memo_access_statistics_update() {
        let mut memo_system = MemoizationSystem::new(1000, 100);
        
        let constraints = SpatialConstraints {
            min_width: Some(100.0),
            max_width: Some(200.0),
            available_width: Some(150.0),
            natural_width: Some(120.0),
            min_height: None,
            max_height: None,
            available_height: None,
            stretch: Some(10.0),
            shrink: Some(5.0),
            line_spacing: Some(1.2),
            paragraph_shape: None,
            math_spacing: None,
            font_metrics: None,
            language_settings: None,
        };
        
        let content_hash = HashComputation::hash_tex_source("\\textbf{Access Test}");
        let state_hash = 33333u128;
        let constraint_hash = HashComputation::hash_constraints(&constraints);
        
        let cache_key = CacheKey {
            content_hash,
            constraint_hash,
            state_hash,
        };
        
        let cache_entry = CacheEntry {
            result_hash: 44444u128,
            constraints: constraints.clone(),
            dependencies: vec![],
            metadata: CacheMetadata {
                source_position: 500,
                context_type: "access_test".to_string(),
                result_size_bytes: 128,
                metrics: EntryMetrics {
                    computation_time_us: 1000,
                    memory_used_bytes: 256,
                    tex_operations: 2,
                    hit_rate: 1.0,
                },
            },
            timestamp: current_timestamp_us(),
            access_count: 0,
            last_access: 0,
        };
        
        memo_system.cache.insert(cache_key.clone(), cache_entry);
        
        // First access
        let result1 = memo_system.check_memo(content_hash, &constraints, state_hash, 500);
        assert!(result1.is_some());
        
        // Verify access count was updated
        let entry_after_first = memo_system.cache.get(&cache_key).unwrap();
        assert_eq!(entry_after_first.access_count, 1);
        assert!(entry_after_first.last_access > 0);
        
        // Second access
        let result2 = memo_system.check_memo(content_hash, &constraints, state_hash, 500);
        assert!(result2.is_some());
        
        // Verify access count was incremented
        let entry_after_second = memo_system.cache.get(&cache_key).unwrap();
        assert_eq!(entry_after_second.access_count, 2);
        
        // Verify multiple hits recorded
        assert_eq!(memo_system.stats.cache_hits, 2);
        assert_eq!(memo_system.stats.total_lookups, 2);
        assert_eq!(memo_system.stats.hit_rate(), 1.0);
    }
    
    // =============================================================================
    // TeX Integration Tests
    // =============================================================================
    
    #[test]
    fn test_check_box_memo_with_tex_context() {
        let mut memo_system = MemoizationSystem::new(1000, 100);
        
        let tex_context = TeXCompilationContext {
            current_position: 100,
            tex_mode: TeXMode::Horizontal,
            nesting_level: 1,
            available_width: Some(400.0),
            available_height: Some(600.0),
            font_properties: FontProperties {
                font_id: 10,
                size_pt: 12.0,
                family_name: "Computer Modern".to_string(),
                char_width: 6.0,
                line_height: 1.2,
                x_height: 4.3,
                baseline_skip: 14.0,
            },
            paragraph_params: ParagraphParams {
                line_width: 400.0,
                left_indent: 0.0,
                right_indent: 0.0,
                first_line_indent: 20.0,
                par_skip: 12.0,
                baseline_skip: 14.0,
                line_spacing: 1.0,
            },
            math_params: None,
        };
        
        let tex_content = "\\textbf{Hello World}";
        
        // First call should be a cache miss
        let result1 = memo_system.check_box_memo(tex_content, &tex_context);
        assert!(result1.is_none());
        assert_eq!(memo_system.stats.cache_misses, 1);
        assert_eq!(memo_system.stats.total_lookups, 1);
        
        // Store a result in cache
        let content_hash = HashComputation::hash_tex_source(tex_content);
        let constraints = memo_system.generate_spatial_constraints_from_context(&tex_context);
        let state_hash = memo_system.generate_comprehensive_state_hash();
        
        let cache_key = CacheKey {
            content_hash,
            constraint_hash: HashComputation::hash_constraints(&constraints),
            state_hash,
        };
        
        let cache_entry = CacheEntry {
            result_hash: 12345u128,
            constraints: constraints.clone(),
            dependencies: vec![],
            metadata: CacheMetadata {
                source_position: 100,
                context_type: "box_test".to_string(),
                result_size_bytes: 256,
                metrics: EntryMetrics {
                    computation_time_us: 1500,
                    memory_used_bytes: 512,
                    tex_operations: 3,
                    hit_rate: 1.0,
                },
            },
            timestamp: current_timestamp_us(),
            access_count: 0,
            last_access: 0,
        };
        
        memo_system.cache.insert(cache_key, cache_entry);
        
        // Second call should be a cache hit
        let result2 = memo_system.check_box_memo(tex_content, &tex_context);
        assert!(result2.is_some());
        assert_eq!(result2.unwrap().result_hash, 12345u128);
        assert_eq!(memo_system.stats.cache_hits, 1);
        assert_eq!(memo_system.stats.total_lookups, 2);
    }
    
    #[test]
    fn test_check_paragraph_memo_with_parameters() {
        let mut memo_system = MemoizationSystem::new(1000, 100);
        
        let tex_context = TeXCompilationContext {
            current_position: 200,
            tex_mode: TeXMode::Horizontal,
            nesting_level: 0,
            available_width: Some(500.0),
            available_height: Some(700.0),
            font_properties: FontProperties {
                font_id: 12,
                size_pt: 11.0,
                family_name: "Times Roman".to_string(),
                char_width: 5.5,
                line_height: 1.15,
                x_height: 4.0,
                baseline_skip: 13.0,
            },
            paragraph_params: ParagraphParams {
                line_width: 480.0,
                left_indent: 20.0,
                right_indent: 20.0,
                first_line_indent: 30.0,
                par_skip: 10.0,
                baseline_skip: 13.0,
                line_spacing: 1.1,
            },
            math_params: None,
        };
        
        let paragraph_content = "This is a test paragraph with some text that should be memoized.";
        
        // Test cache miss
        let result1 = memo_system.check_paragraph_memo(
            paragraph_content,
            &tex_context,
            &tex_context.paragraph_params
        );
        assert!(result1.is_none());
        
        // Test with different paragraph parameters - should also be a miss
        let mut different_params = tex_context.paragraph_params.clone();
        different_params.line_width = 450.0; // Different line width
        
        let result2 = memo_system.check_paragraph_memo(
            paragraph_content,
            &tex_context,
            &different_params
        );
        assert!(result2.is_none());
        assert_eq!(memo_system.stats.cache_misses, 2);
    }
    
    #[test]
    fn test_check_math_memo_inline_vs_display() {
        let mut memo_system = MemoizationSystem::new(1000, 100);
        
        let tex_context = TeXCompilationContext {
            current_position: 300,
            tex_mode: TeXMode::Math { display: false },
            nesting_level: 1,
            available_width: Some(400.0),
            available_height: Some(200.0),
            font_properties: FontProperties {
                font_id: 15,
                size_pt: 12.0,
                family_name: "Computer Modern Math".to_string(),
                char_width: 6.0,
                line_height: 1.2,
                x_height: 4.3,
                baseline_skip: 14.0,
            },
            paragraph_params: ParagraphParams {
                line_width: 400.0,
                left_indent: 0.0,
                right_indent: 0.0,
                first_line_indent: 0.0,
                par_skip: 0.0,
                baseline_skip: 14.0,
                line_spacing: 1.0,
            },
            math_params: Some(MathParams {
                math_surround: 3.0,
                thin_space: 1.5,
                med_space: 2.0,
                thick_space: 2.5,
                display_width: Some(400.0),
                display_indent: 0.0,
            }),
        };
        
        let math_content = "x^2 + y^2 = z^2";
        
        // Test inline math
        let result_inline = memo_system.check_math_memo(math_content, &tex_context, false);
        assert!(result_inline.is_none());
        
        // Test display math - should generate different hash due to display flag
        let result_display = memo_system.check_math_memo(math_content, &tex_context, true);
        assert!(result_display.is_none());
        
        // Verify that inline and display math generate different hashes
        let inline_hash = HashComputation::hash_math_content(math_content, false);
        let display_hash = HashComputation::hash_math_content(math_content, true);
        assert_ne!(inline_hash, display_hash);
        
        assert_eq!(memo_system.stats.cache_misses, 2);
        assert_eq!(memo_system.stats.total_lookups, 2);
    }
    
    #[test]
    fn test_spatial_constraints_generation_from_context() {
        let memo_system = MemoizationSystem::new(1000, 100);
        
        let tex_context = TeXCompilationContext {
            current_position: 150,
            tex_mode: TeXMode::Horizontal,
            nesting_level: 2,
            available_width: Some(450.0),
            available_height: Some(650.0),
            font_properties: FontProperties {
                font_id: 8,
                size_pt: 10.0,
                family_name: "Helvetica".to_string(),
                char_width: 5.0,
                line_height: 1.1,
                x_height: 3.8,
                baseline_skip: 12.0,
            },
            paragraph_params: ParagraphParams {
                line_width: 420.0,
                left_indent: 15.0,
                right_indent: 15.0,
                first_line_indent: 25.0,
                par_skip: 8.0,
                baseline_skip: 12.0,
                line_spacing: 1.05,
            },
            math_params: Some(MathParams {
                math_surround: 2.5,
                thin_space: 1.0,
                med_space: 1.5,
                thick_space: 2.0,
                display_width: Some(400.0),
                display_indent: 10.0,
            }),
        };
        
        let constraints = memo_system.generate_spatial_constraints_from_context(&tex_context);
        
        // Verify width constraints
        assert_eq!(constraints.max_width, Some(450.0));
        assert_eq!(constraints.available_width, Some(450.0));
        
        // Verify height constraints
        assert_eq!(constraints.max_height, Some(650.0));
        assert_eq!(constraints.available_height, Some(650.0));
        
        // Verify line spacing
        assert_eq!(constraints.line_spacing, Some(1.05));
        
        // Verify paragraph shape
        assert!(constraints.paragraph_shape.is_some());
        let para_shape = constraints.paragraph_shape.unwrap();
        assert_eq!(para_shape.left_indent, 15.0);
        assert_eq!(para_shape.right_indent, 15.0);
        assert_eq!(para_shape.first_line_indent, 25.0);
        assert_eq!(para_shape.line_width, 420.0);
        
        // Verify math spacing
        assert!(constraints.math_spacing.is_some());
        let math_spacing = constraints.math_spacing.unwrap();
        assert_eq!(math_spacing.thin_space, 1.0);
        assert_eq!(math_spacing.med_space, 1.5);
        assert_eq!(math_spacing.thick_space, 2.0);
        
        // Verify font metrics
        assert!(constraints.font_metrics.is_some());
        let font_metrics = constraints.font_metrics.unwrap();
        assert_eq!(font_metrics.size, 10.0);
        assert_eq!(font_metrics.family, "Helvetica");
        assert_eq!(font_metrics.char_width, Some(5.0));
        assert_eq!(font_metrics.line_height, 1.1);
        assert_eq!(font_metrics.x_height, 3.8);
        assert_eq!(font_metrics.baseline_skip, 12.0);
        
        // Verify language settings
        assert!(constraints.language_settings.is_some());
        let lang_settings = constraints.language_settings.unwrap();
        assert_eq!(lang_settings.language, "en");
        assert!(lang_settings.hyphenation);
        assert!(lang_settings.ltr);
    }
    
    #[test]
    fn test_paragraph_constraints_generation() {
        let memo_system = MemoizationSystem::new(1000, 100);
        
        let tex_context = TeXCompilationContext {
            current_position: 400,
            tex_mode: TeXMode::Horizontal,
            nesting_level: 1,
            available_width: Some(500.0),
            available_height: Some(800.0),
            font_properties: FontProperties {
                font_id: 20,
                size_pt: 14.0,
                family_name: "Garamond".to_string(),
                char_width: 7.0,
                line_height: 1.3,
                x_height: 5.2,
                baseline_skip: 16.0,
            },
            paragraph_params: ParagraphParams {
                line_width: 480.0,
                left_indent: 25.0,
                right_indent: 25.0,
                first_line_indent: 35.0,
                par_skip: 15.0,
                baseline_skip: 16.0,
                line_spacing: 1.2,
            },
            math_params: None,
        };
        
        let constraints = memo_system.generate_paragraph_constraints(
            &tex_context,
            &tex_context.paragraph_params
        );
        
        // Check paragraph-specific constraints
        assert_eq!(constraints.min_width, Some(240.0)); // 50% of line width
        assert_eq!(constraints.max_width, Some(480.0));
        assert_eq!(constraints.available_width, Some(480.0));
        assert_eq!(constraints.natural_width, Some(480.0));
        
        // Check stretch and shrink allowances
        assert_eq!(constraints.stretch, Some(48.0)); // 10% of line width
        assert_eq!(constraints.shrink, Some(24.0));  // 5% of line width
        
        // Check minimum height is baseline skip
        assert_eq!(constraints.min_height, Some(16.0));
        
        // Math spacing should be None for paragraphs
        assert!(constraints.math_spacing.is_none());
    }
    
    #[test]
    fn test_math_constraints_generation() {
        let memo_system = MemoizationSystem::new(1000, 100);
        
        let tex_context = TeXCompilationContext {
            current_position: 500,
            tex_mode: TeXMode::Math { display: true },
            nesting_level: 1,
            available_width: Some(600.0),
            available_height: Some(400.0),
            font_properties: FontProperties {
                font_id: 25,
                size_pt: 12.0,
                family_name: "Computer Modern Math".to_string(),
                char_width: 6.0,
                line_height: 1.2,
                x_height: 4.3,
                baseline_skip: 14.0,
            },
            paragraph_params: ParagraphParams {
                line_width: 600.0,
                left_indent: 0.0,
                right_indent: 0.0,
                first_line_indent: 0.0,
                par_skip: 0.0,
                baseline_skip: 14.0,
                line_spacing: 1.0,
            },
            math_params: Some(MathParams {
                math_surround: 4.0,
                thin_space: 1.8,
                med_space: 2.4,
                thick_space: 3.0,
                display_width: Some(580.0),
                display_indent: 10.0,
            }),
        };
        
        // Test display math constraints
        let display_constraints = memo_system.generate_math_constraints(&tex_context, true);
        assert_eq!(display_constraints.min_width, Some(290.0)); // 50% of display width
        assert_eq!(display_constraints.max_width, Some(580.0));
        assert_eq!(display_constraints.available_width, Some(580.0));
        
        // Test inline math constraints
        let inline_constraints = memo_system.generate_math_constraints(&tex_context, false);
        assert_eq!(inline_constraints.min_width, Some(12.0)); // 2 * char_width
        assert_eq!(inline_constraints.max_width, Some(600.0)); // available_width
        assert_eq!(inline_constraints.available_width, Some(600.0));
        
        // Both should have math spacing
        assert!(display_constraints.math_spacing.is_some());
        assert!(inline_constraints.math_spacing.is_some());
        
        let math_spacing = display_constraints.math_spacing.unwrap();
        assert_eq!(math_spacing.thin_space, 1.8);
        assert_eq!(math_spacing.med_space, 2.4);
        assert_eq!(math_spacing.thick_space, 3.0);
        assert_eq!(math_spacing.quad_space, 3.6); // thick_space * 1.2
        
        // Math should not have paragraph shape or language settings
        assert!(display_constraints.paragraph_shape.is_none());
        assert!(display_constraints.language_settings.is_none());
        assert!(inline_constraints.paragraph_shape.is_none());
        assert!(inline_constraints.language_settings.is_none());
        
        // Math expressions don't stretch or shrink
        assert!(display_constraints.stretch.is_none());
        assert!(display_constraints.shrink.is_none());
        assert!(inline_constraints.stretch.is_none());
        assert!(inline_constraints.shrink.is_none());
    }
    
    #[test]
    fn test_comprehensive_state_hash_generation() {
        let mut memo_system = MemoizationSystem::new(1000, 100);
        
        // Get initial state hash
        let initial_hash = memo_system.generate_comprehensive_state_hash();
        
        // Define a macro
        memo_system.define_macro(
            "testmacro".to_string(),
            "\\textbf{#1}".to_string(),
            1
        );
        
        // State hash should change after macro definition
        let hash_after_macro = memo_system.generate_comprehensive_state_hash();
        assert_ne!(initial_hash, hash_after_macro);
        
        // Set a register
        memo_system.set_count_register(42, 100, Some(123));
        
        // State hash should change after register change
        let hash_after_register = memo_system.generate_comprehensive_state_hash();
        assert_ne!(hash_after_macro, hash_after_register);
        
        // Getting the same state twice should produce same hash
        let hash_repeat = memo_system.generate_comprehensive_state_hash();
        assert_eq!(hash_after_register, hash_repeat);
    }
    
    #[test]
    fn test_tex_mode_enum_variants() {
        // Test TeX mode enumeration
        let horizontal = TeXMode::Horizontal;
        let vertical = TeXMode::Vertical;
        let inline_math = TeXMode::Math { display: false };
        let display_math = TeXMode::Math { display: true };
        let restricted = TeXMode::RestrictedHorizontal;
        let internal = TeXMode::InternalVertical;
        
        assert_eq!(horizontal, TeXMode::Horizontal);
        assert_eq!(vertical, TeXMode::Vertical);
        assert_eq!(inline_math, TeXMode::Math { display: false });
        assert_eq!(display_math, TeXMode::Math { display: true });
        assert_eq!(restricted, TeXMode::RestrictedHorizontal);
        assert_eq!(internal, TeXMode::InternalVertical);
        
        // Verify different math modes are distinct
        assert_ne!(inline_math, display_math);
    }
    
    #[test]
    fn test_update_box_building_context() {
        let mut memo_system = MemoizationSystem::new(1000, 100);
        
        // This method is currently a placeholder but should not panic
        memo_system.update_box_building_context(
            450.0, // hsize
            650.0, // vsize
            12,    // current font
            14.0   // baseline skip
        );
        
        // Test notification methods
        memo_system.notify_paragraph_break(1000);
        memo_system.notify_page_break(1, 2000);
        
        // These should not cause any issues
    }
}

// =============================================================================
// Hash Computation Implementation (Phase 2.3)
// =============================================================================

/// Hash computation utilities for memoization system
/// Uses SHA-256 for content hashing and SipHash-128 for compact storage
pub struct HashComputation;

impl HashComputation {
    /// Create content hash using SHA-256 for any serializable content
    pub fn hash_content<T: Serialize>(content: &T) -> u128 {
        let serialized = bincode::serialize(content).unwrap_or_default();
        let mut hasher = Sha256::new();
        hasher.update(&serialized);
        let result = hasher.finalize();
        
        // Convert SHA-256 result to u128 by taking first 16 bytes
        let mut bytes = [0u8; 16];
        bytes.copy_from_slice(&result[..16]);
        u128::from_le_bytes(bytes)
    }
    
    /// Hash TeX source content with normalization
    pub fn hash_tex_source(source: &str) -> u128 {
        // Normalize whitespace for consistent hashing
        let normalized = source
            .lines()
            .map(|line| line.trim_end())
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        
        let mut hasher = SipHasher128::new();
        normalized.hash(&mut hasher);
        hasher.finish128()
    }
    
    /// Hash paragraph content with formatting parameters
    pub fn hash_paragraph_content(content: &str, params: &ParagraphParams) -> u128 {
        let mut hasher = SipHasher128::new();
        
        // Hash normalized content
        Self::hash_tex_source(content).hash(&mut hasher);
        
        // Include paragraph parameters in hash
        params.line_width.to_bits().hash(&mut hasher);
        params.left_indent.to_bits().hash(&mut hasher);
        params.right_indent.to_bits().hash(&mut hasher);
        params.first_line_indent.to_bits().hash(&mut hasher);
        params.par_skip.to_bits().hash(&mut hasher);
        params.baseline_skip.to_bits().hash(&mut hasher);
        params.line_spacing.to_bits().hash(&mut hasher);
        
        hasher.finish128()
    }
    
    /// Hash math content with display mode flag
    pub fn hash_math_content(content: &str, is_display: bool) -> u128 {
        let mut hasher = SipHasher128::new();
        
        // Hash math content (whitespace sensitive in math mode)
        content.hash(&mut hasher);
        is_display.hash(&mut hasher);
        
        hasher.finish128()
    }
    
    /// Hash spatial constraints for cache lookup
    pub fn hash_constraints(constraints: &SpatialConstraints) -> u128 {
        let mut hasher = SipHasher128::new();
        
        // Hash dimension constraints
        if let Some(v) = constraints.min_width {
            v.to_bits().hash(&mut hasher);
        }
        if let Some(v) = constraints.max_width {
            v.to_bits().hash(&mut hasher);
        }
        if let Some(v) = constraints.available_width {
            v.to_bits().hash(&mut hasher);
        }
        if let Some(v) = constraints.min_height {
            v.to_bits().hash(&mut hasher);
        }
        if let Some(v) = constraints.max_height {
            v.to_bits().hash(&mut hasher);
        }
        if let Some(v) = constraints.available_height {
            v.to_bits().hash(&mut hasher);
        }
        
        // Hash box constraints
        if let Some(v) = constraints.natural_width {
            v.to_bits().hash(&mut hasher);
        }
        if let Some(v) = constraints.stretch {
            v.to_bits().hash(&mut hasher);
        }
        if let Some(v) = constraints.shrink {
            v.to_bits().hash(&mut hasher);
        }
        
        // Hash TeX-specific constraints
        if let Some(v) = constraints.line_spacing {
            v.to_bits().hash(&mut hasher);
        }
        
        if let Some(ref shape) = constraints.paragraph_shape {
            shape.left_indent.to_bits().hash(&mut hasher);
            shape.right_indent.to_bits().hash(&mut hasher);
            shape.first_line_indent.to_bits().hash(&mut hasher);
            shape.line_width.to_bits().hash(&mut hasher);
            shape.hanging_indent.to_bits().hash(&mut hasher);
        }
        
        if let Some(ref spacing) = constraints.math_spacing {
            spacing.thin_space.to_bits().hash(&mut hasher);
            spacing.med_space.to_bits().hash(&mut hasher);
            spacing.thick_space.to_bits().hash(&mut hasher);
            spacing.quad_space.to_bits().hash(&mut hasher);
        }
        
        hasher.finish128()
    }
    
    /// Hash macro definitions for state tracking
    pub fn hash_macro_definitions(macros: &BTreeMap<String, (String, usize)>) -> u128 {
        let mut hasher = SipHasher128::new();
        
        for (name, (definition, arg_count)) in macros {
            name.hash(&mut hasher);
            definition.hash(&mut hasher);
            arg_count.hash(&mut hasher);
        }
        
        hasher.finish128()
    }
    
    /// Hash macro expansion with tracking of nested expansions
    pub fn hash_macro_expansion(
        macro_name: &str, 
        args: &[String], 
        expansion_depth: u32
    ) -> u128 {
        let mut hasher = SipHasher128::new();
        
        // Hash macro name
        macro_name.hash(&mut hasher);
        
        // Hash each argument
        for arg in args {
            arg.hash(&mut hasher);
        }
        
        // Include expansion depth to track nested macro calls
        expansion_depth.hash(&mut hasher);
        
        hasher.finish128()
    }
    
    /// Hash font state including all active font properties
    pub fn hash_font_state(font_props: &FontProperties) -> u128 {
        let mut hasher = SipHasher128::new();
        
        // Hash all font properties that affect rendering
        font_props.font_id.hash(&mut hasher);
        font_props.size_pt.to_bits().hash(&mut hasher);
        font_props.family_name.hash(&mut hasher);
        font_props.char_width.to_bits().hash(&mut hasher);
        font_props.line_height.to_bits().hash(&mut hasher);
        font_props.x_height.to_bits().hash(&mut hasher);
        font_props.baseline_skip.to_bits().hash(&mut hasher);
        
        hasher.finish128()
    }
    
    /// Hash TeX counter states (count, dimen, skip registers)
    pub fn hash_counter_states(
        count_registers: &HashMap<i32, i32>,
        dimen_registers: &HashMap<i32, f64>,
        skip_registers: &HashMap<i32, (f64, f64, f64)>
    ) -> u128 {
        let mut hasher = SipHasher128::new();
        
        // Sort and hash count registers for consistent ordering
        let mut count_vec: Vec<_> = count_registers.iter().collect();
        count_vec.sort_by_key(|&(k, _)| k);
        for (reg, value) in count_vec {
            reg.hash(&mut hasher);
            value.hash(&mut hasher);
        }
        
        // Sort and hash dimen registers
        let mut dimen_vec: Vec<_> = dimen_registers.iter().collect();
        dimen_vec.sort_by_key(|&(k, _)| k);
        for (reg, value) in dimen_vec {
            reg.hash(&mut hasher);
            value.to_bits().hash(&mut hasher);
        }
        
        // Sort and hash skip registers (glue)
        let mut skip_vec: Vec<_> = skip_registers.iter().collect();
        skip_vec.sort_by_key(|&(k, _)| k);
        for (reg, (base, stretch, shrink)) in skip_vec {
            reg.hash(&mut hasher);
            base.to_bits().hash(&mut hasher);
            stretch.to_bits().hash(&mut hasher);
            shrink.to_bits().hash(&mut hasher);
        }
        
        hasher.finish128()
    }
    
    /// Hash comprehensive TeX engine state for memoization
    pub fn hash_tex_engine_state(
        tex_state: &TeXEngineState,
        font_state: &FontProperties,
        macros: &BTreeMap<String, (String, usize)>,
        count_registers: &HashMap<i32, i32>,
        dimen_registers: &HashMap<i32, f64>,
        skip_registers: &HashMap<i32, (f64, f64, f64)>
    ) -> u128 {
        let mut hasher = SipHasher128::new();
        
        // Hash core TeX state
        Self::hash_content(tex_state).hash(&mut hasher);
        
        // Hash font state
        Self::hash_font_state(font_state).hash(&mut hasher);
        
        // Hash macro definitions
        Self::hash_macro_definitions(macros).hash(&mut hasher);
        
        // Hash counter states
        Self::hash_counter_states(count_registers, dimen_registers, skip_registers).hash(&mut hasher);
        
        hasher.finish128()
    }
}