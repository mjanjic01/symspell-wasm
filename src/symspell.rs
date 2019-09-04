use std::cmp;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fs::File;
use std::hash::{Hash, Hasher};
use std::i64;
use std::io::{BufRead, BufReader};
use std::path::Path;
use unidecode::unidecode;

use edit_distance::{DistanceAlgorithm, EditDistance};
use string_strategy::StringStrategy;
use suggestion::Suggestion;

#[derive(Eq, PartialEq, Debug)]
pub enum Verbosity {
    Top,
    Closest,
    All,
}

#[derive(Builder, PartialEq)]
pub struct SymSpell<T: StringStrategy> {
    /// Maximum edit distance for doing lookups.
    #[builder(default = "2")]
    max_dictionary_edit_distance: i64,
    /// The length of word prefixes used for spell checking.
    #[builder(default = "7")]
    prefix_length: i64,
    /// The minimum frequency count for dictionary words to be considered correct spellings.
    #[builder(default = "1")]
    count_threshold: i64,

    #[builder(default = "0", setter(skip))]
    max_length: i64,
    #[builder(default = "HashMap::new()", setter(skip))]
    deletes: HashMap<u64, Vec<String>>,
    #[builder(default = "HashMap::new()", setter(skip))]
    words: HashMap<String, i64>,
    #[builder(default = "DistanceAlgorithm::Damerau")]
    distance_algorithm: DistanceAlgorithm,
    #[builder(default = "T::new()", setter(skip))]
    string_strategy: T,
}

impl<T: StringStrategy> Default for SymSpell<T> {
    fn default() -> SymSpell<T> {
        SymSpellBuilder::default().build().unwrap()
    }
}

impl<T: StringStrategy> SymSpell<T> {
    /// Load multiple dictionary entries from a file of word/frequency count pairs.
    ///
    /// # Arguments
    ///
    /// * `corpus` - The path+filename of the file.
    /// * `term_index` - The column position of the word.
    /// * `count_index` - The column position of the frequency count.
    /// * `separator` - Separator between word and frequency
    pub fn load_dictionary(
        &mut self,
        corpus: &str,
        term_index: i64,
        count_index: i64,
        separator: &str,
    ) -> bool {
        if !Path::new(corpus).exists() {
            return false;
        }

        let file = File::open(corpus).expect("file not found");
        let sr = BufReader::new(file);

        for (i, line) in sr.lines().enumerate() {
            if i % 50_000 == 0 {
                println!("progress: {}", i);
            }
            let line_str = line.unwrap();
            let line_parts: Vec<&str> = line_str.split(separator).collect();

            if line_parts.len() >= 2 {
                // let key = unidecode(line_parts[term_index as usize]);
                let key = self
                    .string_strategy
                    .prepare(line_parts[term_index as usize]);
                let count = line_parts[count_index as usize].parse::<i64>().unwrap();

                self.create_dictionary_entry(key, count);
            }
        }

        true
    }

    /// Load multiple dictionary entries from a file of word/frequency count pairs.
    ///
    /// # Arguments
    ///
    /// * `input` - Full dictionary as a string.
    /// * `term_index` - The column position of the word.
    /// * `count_index` - The column position of the frequency count.
    /// * `separator` - Separator between word and frequency
    pub fn load_dictionary_from_string(
        &mut self,
        input: &str,
        term_index: i64,
        count_index: i64,
        separator: &str,
    ) -> bool {
        for (i, line) in input.lines().enumerate() {
            if i % 50_000 == 0 {
                println!("progress: {}", i);
            }
            let line_str = line;
            let line_parts: Vec<&str> = line_str.split(separator).collect();

            if line_parts.len() >= 2 {
                // let key = unidecode(line_parts[term_index as usize]);
                let key = self
                    .string_strategy
                    .prepare(line_parts[term_index as usize]);
                let count = line_parts[count_index as usize].parse::<i64>().unwrap();

                self.create_dictionary_entry(key, count);
            }
        }

        true
    }

    /// Find suggested spellings for a given input word, using the maximum
    /// edit distance specified during construction of the SymSpell dictionary.
    ///
    /// # Arguments
    ///
    /// * `input` - The word being spell checked.
    /// * `verbosity` - The value controlling the quantity/closeness of the retuned suggestions.
    /// * `max_edit_distance` - The maximum edit distance between input and suggested words.
    ///
    /// # Examples
    ///
    /// ```
    /// let mut symspell: SymSpell<AsciiStringStrategy> = SymSpell::default();
    /// symspell.load_dictionary("data/frequency_dictionary_en_82_765.txt", 0, 1, " ");
    /// symspell.lookup("whatver", Verbosity::Top, 2)
    /// ```
    pub fn lookup(
        &self,
        input: &str,
        verbosity: Verbosity,
        max_edit_distance: i64,
    ) -> Vec<Suggestion> {
        if max_edit_distance > self.max_dictionary_edit_distance {
            panic!("max_edit_distance is bigger than max_dictionary_edit_distance");
        }

        let mut suggestions: Vec<Suggestion> = Vec::new();

        let input = &unidecode(input);
        let input_len = self.string_strategy.len(input) as i64;

        if input_len - self.max_dictionary_edit_distance > self.max_length {
            return suggestions;
        }

        let mut hashset1: HashSet<String> = HashSet::new();
        let mut hashset2: HashSet<String> = HashSet::new();

        if self.words.contains_key(input) {
            let suggestion_count = self.words[input];
            suggestions.push(Suggestion::new(input, 0, suggestion_count));

            if verbosity != Verbosity::All {
                return suggestions;
            }
        }

        hashset2.insert(input.to_string());

        let mut max_edit_distance2 = max_edit_distance;
        let mut candidate_pointer = 0;
        let mut candidates = Vec::new();

        let mut input_prefix_len = input_len;

        if input_prefix_len > self.prefix_length {
            input_prefix_len = self.prefix_length;
            candidates.push(
                self.string_strategy
                    .slice(input, 0, input_prefix_len as usize),
            );
        } else {
            candidates.push(input.to_string());
        }

        let distance_comparer = EditDistance::new(self.distance_algorithm.clone());

        while candidate_pointer < candidates.len() {
            let candidate = &candidates.get(candidate_pointer).unwrap().clone();
            candidate_pointer += 1;
            let candidate_len = self.string_strategy.len(candidate) as i64;
            let length_diff = input_prefix_len - candidate_len;

            if length_diff > max_edit_distance2 {
                if verbosity == Verbosity::All {
                    continue;
                }
                break;
            }

            if self.deletes.contains_key(&self.get_string_hash(&candidate)) {
                let dict_suggestions = &self.deletes[&self.get_string_hash(&candidate)];

                for suggestion in dict_suggestions {
                    let suggestion_len = self.string_strategy.len(suggestion) as i64;

                    if suggestion == input {
                        continue;
                    }

                    if (suggestion_len - input_len).abs() > max_edit_distance2
                        || suggestion_len < candidate_len
                        || (suggestion_len == candidate_len && suggestion != candidate)
                    {
                        continue;
                    }

                    let sugg_prefix_len = cmp::min(suggestion_len, self.prefix_length);

                    if sugg_prefix_len > input_prefix_len
                        && sugg_prefix_len - candidate_len > max_edit_distance2
                    {
                        continue;
                    }

                    let distance;

                    if candidate_len == 0 {
                        distance = cmp::max(input_len, suggestion_len);

                        if distance > max_edit_distance2 || hashset2.contains(suggestion) {
                            continue;
                        }
                        hashset2.insert(suggestion.to_string());
                    } else if suggestion_len == 1 {
                        distance = if !input.contains(&self.string_strategy.slice(suggestion, 0, 1))
                        {
                            input_len
                        } else {
                            input_len - 1
                        };

                        if distance > max_edit_distance2 || hashset2.contains(suggestion) {
                            continue;
                        }

                        hashset2.insert(suggestion.to_string());
                    } else if self.has_different_suffix(
                        max_edit_distance,
                        input,
                        input_len,
                        candidate_len,
                        suggestion,
                        suggestion_len,
                    ) {
                        continue;
                    } else {
                        if verbosity != Verbosity::All
                            && !self.delete_in_suggestion_prefix(
                                candidate,
                                candidate_len,
                                suggestion,
                                suggestion_len,
                            )
                        {
                            continue;
                        }

                        if hashset2.contains(suggestion) {
                            continue;
                        }
                        hashset2.insert(suggestion.to_string());

                        distance = distance_comparer.compare(input, suggestion, max_edit_distance2);

                        if distance < 0 {
                            continue;
                        }
                    }

                    if distance <= max_edit_distance2 {
                        let suggestion_count = self.words[suggestion];
                        let si = Suggestion::new(suggestion, distance, suggestion_count);

                        if suggestions.len() > 0 {
                            match verbosity {
                                Verbosity::Closest => {
                                    if distance < max_edit_distance2 {
                                        suggestions.clear();
                                    }
                                }
                                Verbosity::Top => {
                                    if distance < max_edit_distance2
                                        || suggestion_count > suggestions[0].count
                                    {
                                        max_edit_distance2 = distance;
                                        suggestions[0] = si;
                                    }
                                    continue;
                                }
                                _ => (),
                            }
                        }

                        if verbosity != Verbosity::All {
                            max_edit_distance2 = distance;
                        }

                        suggestions.push(si);
                    }
                }
            }

            if length_diff < max_edit_distance && candidate_len <= self.prefix_length {
                if verbosity != Verbosity::All && length_diff >= max_edit_distance2 {
                    continue;
                }

                for i in 0..candidate_len {
                    let delete = self.string_strategy.remove(candidate, i as usize);

                    if !hashset1.contains(&delete) {
                        hashset1.insert(delete.clone());
                        candidates.push(delete);
                    }
                }
            }
        }

        if suggestions.len() > 1 {
            suggestions.sort();
        }

        suggestions
    }

    /// Find suggested spellings for a given input sentence, using the maximum
    /// edit distance specified during construction of the SymSpell dictionary.
    ///
    /// # Arguments
    ///
    /// * `input` - The sentence being spell checked.
    /// * `max_edit_distance` - The maximum edit distance between input and suggested words.
    ///
    /// # Examples
    ///
    /// ```
    /// let mut symspell: SymSpell<AsciiStringStrategy> = SymSpell::default();
    /// symspell.load_dictionary("data/frequency_dictionary_en_82_765.txt", 0, 1, " ");
    /// symspell.lookup_compound("whereis th elove", 2)
    /// ```
    pub fn lookup_compound(&self, input: &str, edit_distance_max: i64) -> Vec<Suggestion> {
        //parse input string into single terms
        let term_list1 = self.parse_words(&self.string_strategy.prepare(input));

        // let mut suggestions_previous_term: Vec<Suggestion> = Vec::new();                  //suggestions for a single term
        let mut suggestions: Vec<Suggestion>;
        let mut suggestion_parts: Vec<Suggestion> = Vec::new();
        let distance_comparer = EditDistance::new(self.distance_algorithm.clone());

        //translate every term to its best suggestion, otherwise it remains unchanged
        let mut last_combi = false;

        for (i, term) in term_list1.iter().enumerate() {
            suggestions = self.lookup(term, Verbosity::Top, edit_distance_max);

            //combi check, always before split
            if i > 0 && !last_combi {
                let mut suggestions_combi: Vec<Suggestion> = self.lookup(
                    &format!("{}{}", term_list1[i - 1], term_list1[i]),
                    Verbosity::Top,
                    edit_distance_max,
                );

                if suggestions_combi.len() > 0 {
                    let best1 = suggestion_parts[suggestion_parts.len() - 1].clone();
                    let mut best2 = Suggestion::empty();

                    if suggestions.len() > 0 {
                        best2 = suggestions[0].clone();
                    } else {
                        best2.term = term_list1[i].clone();
                        best2.distance = edit_distance_max + 1;
                        best2.count = 0;
                    }
                    //if (suggestions_combi[0].distance + 1 < DamerauLevenshteinDistance(term_list1[i - 1] + " " + term_list1[i], best1.term + " " + best2.term))
                    let distance1 = distance_comparer.compare(
                        &format!("{} {}", term_list1[i - 1], term_list1[i]),
                        &format!("{} {}", best1.term, best2.term),
                        edit_distance_max,
                    );

                    if (distance1 >= 0) && (suggestions_combi[0].distance + 1 < distance1) {
                        suggestions_combi[0].distance += 1;
                        let last_i = suggestion_parts.len() - 1;
                        suggestion_parts[last_i] = suggestions_combi[0].clone();
                        last_combi = true;
                        continue;
                    }
                }
            }
            last_combi = false;

            //alway split terms without suggestion / never split terms with suggestion ed=0 / never split single char terms
            if (suggestions.len() > 0)
                && ((suggestions[0].distance == 0)
                    || (self.string_strategy.len(&term_list1[i]) == 1))
            {
                //choose best suggestion
                suggestion_parts.push(suggestions[0].clone());
            } else {
                //if no perfect suggestion, split word into pairs
                let mut suggestions_split: Vec<Suggestion> = Vec::new();

                //add original term
                if suggestions.len() > 0 {
                    suggestions_split.push(suggestions[0].clone());
                }

                let term_length = self.string_strategy.len(&term_list1[i]);

                if term_length > 1 {
                    for j in 1..term_length {
                        let part1 = self.string_strategy.slice(&term_list1[i], 0, j);
                        let part2 = self.string_strategy.slice(&term_list1[i], j, term_length);

                        let mut suggestion_split = Suggestion::empty();

                        let suggestions1 = self.lookup(&part1, Verbosity::Top, edit_distance_max);

                        if suggestions1.len() > 0 {
                            if (suggestions.len() > 0)
                                && (suggestions[0].term == suggestions1[0].term)
                            {
                                break;
                            } //if split correction1 == einzelwort correction

                            let suggestions2 =
                                self.lookup(&part2, Verbosity::Top, edit_distance_max);

                            if suggestions2.len() > 0 {
                                if (suggestions.len() > 0)
                                    && (suggestions[0].term == suggestions2[0].term)
                                {
                                    break;
                                } //if split correction1 == einzelwort correction

                                //select best suggestion for split pair
                                suggestion_split.term =
                                    format!("{} {}", suggestions1[0].term, suggestions2[0].term);

                                let mut distance2 = distance_comparer.compare(
                                    &term_list1[i],
                                    &format!("{} {}", suggestions1[0].term, suggestions2[0].term),
                                    edit_distance_max,
                                );

                                if distance2 < 0 {
                                    distance2 = edit_distance_max + 1;
                                }

                                suggestion_split.distance = distance2;
                                suggestion_split.count =
                                    cmp::min(suggestions1[0].count, suggestions2[0].count);
                                suggestions_split.push(suggestion_split.clone());

                                //early termination of split
                                if suggestion_split.distance == 1 {
                                    break;
                                }
                            }
                        }
                    }

                    if suggestions_split.len() > 0 {
                        //select best suggestion for split pair
                        suggestions_split.sort_by(|x, y| {
                            x.distance
                                .cmp(&y.distance)
                                .then(x.count.cmp(&y.count).reverse())
                        });
                        suggestion_parts.push(suggestions_split[0].clone());
                    } else {
                        let mut si = Suggestion::empty();
                        si.term = term_list1[i].clone();
                        si.count = 0;
                        si.distance = edit_distance_max + 1;
                        suggestion_parts.push(si);
                    }
                } else {
                    let mut si = Suggestion::empty();
                    si.term = term_list1[i].clone();
                    si.count = 0;
                    si.distance = edit_distance_max + 1;
                    suggestion_parts.push(si);
                }
            }
        }

        let mut suggestion = Suggestion::empty();

        suggestion.count = i64::MAX;

        let mut s = "".to_string();

        for si in suggestion_parts {
            s.push_str(&si.term);
            s.push_str(" ");
            suggestion.count = cmp::min(suggestion.count, si.count);
        }

        suggestion.term = s.trim().to_string();
        suggestion.distance = distance_comparer.compare(input, &suggestion.term, i64::MAX);

        vec![suggestion]
    }

    fn delete_in_suggestion_prefix(
        &self,
        delete: &str,
        delete_len: i64,
        suggestion: &str,
        suggestion_len: i64,
    ) -> bool {
        if delete_len == 0 {
            return true;
        }
        let suggestion_len = if self.prefix_length < suggestion_len {
            self.prefix_length
        } else {
            suggestion_len
        };
        let mut j = 0;
        for i in 0..delete_len {
            let del_char = self.string_strategy.at(delete, i as isize).unwrap();
            while j < suggestion_len
                && del_char != self.string_strategy.at(suggestion, j as isize).unwrap()
            {
                j += 1;
            }

            if j == suggestion_len {
                return false;
            }
        }
        true
    }

    fn create_dictionary_entry(&mut self, key: String, count: i64) -> bool {
        if count < self.count_threshold {
            return false;
        }

        self.words.insert(key.clone(), count);

        let key_len = self.string_strategy.len(&key);

        if key_len as i64 > self.max_length {
            self.max_length = key_len as i64;
        }

        let edits = self.edits_prefix(&key);

        for delete in edits {
            let delete_hash = self.get_string_hash(&delete);

            if self.deletes.contains_key(&delete_hash) {
                let suggestions = self.deletes.get_mut(&delete_hash).unwrap();
                suggestions.push(key.clone());
            } else {
                self.deletes.insert(delete_hash, vec![key.to_string()]);
            }
        }

        true
    }

    fn edits_prefix(&self, key: &str) -> HashSet<String> {
        let mut hash_set = HashSet::new();

        let key_len = self.string_strategy.len(key) as i64;

        if key_len <= self.max_dictionary_edit_distance {
            hash_set.insert("".to_string());
        }

        if key_len > self.prefix_length {
            let shortened_key = self
                .string_strategy
                .slice(key, 0, self.prefix_length as usize);
            hash_set.insert(shortened_key.clone());
            self.edits(&shortened_key, 0, &mut hash_set);
        } else {
            hash_set.insert(key.to_string());
            self.edits(key, 0, &mut hash_set);
        };

        hash_set
    }

    fn edits(&self, word: &str, edit_distance: i64, delete_words: &mut HashSet<String>) {
        let edit_distance = edit_distance + 1;
        let word_len = self.string_strategy.len(word);

        if word_len > 1 {
            for i in 0..word_len {
                let delete = self.string_strategy.remove(word, i);

                if !delete_words.contains(&delete) {
                    delete_words.insert(delete.clone());

                    if edit_distance < self.max_dictionary_edit_distance {
                        self.edits(&delete, edit_distance, delete_words);
                    }
                }
            }
        }
    }

    fn has_different_suffix(
        &self,
        max_edit_distance: i64,
        input: &str,
        input_len: i64,
        candidate_len: i64,
        suggestion: &str,
        suggestion_len: i64,
    ) -> bool {
        let min = cmp::min(input_len, suggestion_len) as i64 - self.prefix_length;

        (self.prefix_length - max_edit_distance == candidate_len)
            && (((min - self.prefix_length) > 1)
                && (self
                    .string_strategy
                    .suffix(input, (input_len + 1 - min) as usize)
                    != self
                        .string_strategy
                        .suffix(suggestion, (suggestion_len + 1 - min) as usize)))
            || ((min > 0)
                && (self.string_strategy.at(input, (input_len - min) as isize)
                    != self
                        .string_strategy
                        .at(suggestion, (suggestion_len - min) as isize))
                && ((self
                    .string_strategy
                    .at(input, (input_len - min - 1) as isize)
                    != self
                        .string_strategy
                        .at(suggestion, (suggestion_len - min) as isize))
                    || (self.string_strategy.at(input, (input_len - min) as isize)
                        != self
                            .string_strategy
                            .at(suggestion, (suggestion_len - min - 1) as isize))))
    }

    fn get_string_hash(&self, s: &String) -> u64 {
        let mut hasher = DefaultHasher::new();
        s.hash(&mut hasher);
        hasher.finish()
    }

    fn parse_words(&self, text: &str) -> Vec<String> {
        text.to_lowercase()
            .split_whitespace()
            .map(|s| s.to_string())
            .collect()
    }
}
