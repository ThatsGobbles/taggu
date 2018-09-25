use std::path::Path;
use std::path::PathBuf;
use std::collections::HashMap;
use std::collections::hash_map::Entry;

use failure::Error;

use library::config::Config;
use metadata::types::MetaBlock;
use metadata::location::MetaLocation;
use metadata::reader::MetaReader;
use metadata::plexer::MetaPlexer;

pub struct MetaProcessor;

impl MetaProcessor {
    pub fn process_meta_file<MR, P>(
        meta_path: P,
        meta_location: MetaLocation,
        config: &Config,
    ) -> Result<HashMap<PathBuf, MetaBlock>, Error>
    where
        MR: MetaReader,
        P: AsRef<Path>,
    {
        let meta_structure = MR::from_file(&meta_path, meta_location)?;

        let item_paths = meta_location.get_configured_item_paths(&meta_path, config)?;

        let meta_plexed = MetaPlexer::plex(meta_structure, item_paths);

        Ok(meta_plexed)
    }

    pub fn process_item_file<MR, P>(
        item_path: P,
        meta_location: MetaLocation,
        config: &Config,
    ) -> Result<MetaBlock, Error>
    where
        MR: MetaReader,
        P: AsRef<Path>,
    {
        let meta_path = meta_location.get_meta_path(&item_path)?;

        let mut processed_meta_file = Self::process_meta_file::<MR, _>(&meta_path, meta_location, config)?;

        // The remaining results can be thrown away.
        if let Some(meta_block) = processed_meta_file.remove(item_path.as_ref()) {
            Ok(meta_block)
        }
        else {
            bail!("item path not found in processed metadata: \"{}\"", item_path.as_ref().to_string_lossy());
        }
    }

    pub fn process_meta_file_cached<'c, MR, P>(
        meta_path: P,
        meta_location: MetaLocation,
        config: &Config,
        cache: &'c mut HashMap<PathBuf, HashMap<PathBuf, MetaBlock>>,
        force: bool,
    ) -> Result<&'c HashMap<PathBuf, MetaBlock>, Error>
    where
        MR: MetaReader,
        P: AsRef<Path>,
    {
        let meta_path = meta_path.as_ref();

        if force {
            cache.remove(meta_path);
        }

        let meta_file_results = match cache.entry(meta_path.to_owned()) {
            Entry::Occupied(e) => e.into_mut(),
            Entry::Vacant(e) => e.insert(Self::process_meta_file::<MR, _>(meta_path, meta_location, config)?),
        };

        Ok(meta_file_results)
    }

    pub fn process_item_file_cached<'c, MR, P>(
        item_path: P,
        meta_location: MetaLocation,
        config: &Config,
        cache: &'c mut HashMap<PathBuf, HashMap<PathBuf, MetaBlock>>,
        force: bool,
    ) -> Result<&'c MetaBlock, Error>
    where
        MR: MetaReader,
        P: AsRef<Path>,
    {
        let meta_path = meta_location.get_meta_path(&item_path)?;

        let processed_meta_file = Self::process_meta_file_cached::<MR, _>(&meta_path, meta_location, config, cache, force)?;
        processed_meta_file.get(item_path.as_ref())
            .ok_or(bail!("item path not found in processed metadata: \"{}\"", item_path.as_ref().to_string_lossy()))
    }
}

#[cfg(test)]
mod tests {
    use super::MetaProcessor;

    use failure::Error;

    use library::config::Config;
    use metadata::reader::MetaReader;
    use metadata::reader::yaml::YamlMetaReader;
    use metadata::location::MetaLocation;
    use metadata::structure::MetaStructure;
    use metadata::types::MetaVal;

    use test_util::create_temp_media_test_dir;

    #[test]
    fn test_process_meta_file() {
        let temp_dir = create_temp_media_test_dir("test_process_meta_file");
        let path = temp_dir.path();

        let config = Config::default();

        let inputs_and_expected = vec![
            (
                (path.join("self.yml"), MetaLocation::Contains),
                hashmap![
                    path.to_owned() => btreemap![
                        "ROOT_self_key".to_owned() => MetaVal::Str("ROOT_self_val".to_owned()),
                        "self_key".to_owned() => MetaVal::Str("self_val".to_owned()),
                        "const_key".to_owned() => MetaVal::Str("const_val".to_owned()),
                    ],
                ],
            ),
        ];

        for (input, expected) in inputs_and_expected {
            let (meta_path, meta_location) = input;

            let produced = MetaProcessor::process_meta_file::<YamlMetaReader, _>(meta_path, meta_location, &config).unwrap();
            assert_eq!(expected, produced);
        }

        let result = MetaProcessor::process_meta_file::<YamlMetaReader, _>(path.join("self.yml"), MetaLocation::Contains, &config);

        println!("{:?}", result);
    }
}
