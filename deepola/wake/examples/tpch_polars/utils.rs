extern crate wake;
use glob::glob;
use polars::export::chrono::NaiveDate;
use polars::prelude::*;
use serde::Serialize;
use std::collections::HashMap;
use std::error::Error;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::time::Instant;
use wake::data::*;
use wake::graph::*;
use wake::polars_operations::*;

#[derive(Debug)]
pub struct TableInput {
    pub input_files: Vec<String>,
    pub scale: usize,
}

pub const FILE_FORMAT_CSV: &str = ".tbl";
pub const FILE_FORMAT_PARQUET: &str = ".parquet";
pub const FILE_FORMAT_DEFAULT: &str = FILE_FORMAT_PARQUET;

// Helper function to compute number of days since epoch
pub fn days_since_epoch(year: i32, month: u32, day: u32) -> i32 {
    let given_date = NaiveDate::from_ymd(year, month, day);
    let epoch_date = NaiveDate::from_ymd(1970,1,1);
    NaiveDate::signed_duration_since(given_date, epoch_date).num_days().try_into().unwrap()
}

pub fn total_number_of_records(table: &str, scale: usize) -> usize {
    match table {
        "lineitem" => 6_000_000 * scale,
        "orders" => 1_500_000 * scale,
        "part" => 200_000 * scale,
        "partsupp" => 800_000 * scale,
        "customer" => 150_000 * scale,
        "supplier" => 10_000 * scale,
        "nation" => 25,
        "region" => 5,
        _ => 0,
    }
}

pub fn load_tables(directory: &str, scale: usize) -> HashMap<String, TableInput> {
    log::warn!("Specified Input Directory: {}", directory);

    let tpch_tables = vec![
        "lineitem", "orders", "customer", "part", "partsupp", "region", "nation", "supplier",
    ];
    let mut table_input = HashMap::new();
    for tpch_table in tpch_tables {
        let mut input_files = vec![];
        for entry in
            glob(&format!("{}/{}.*", directory, tpch_table)).expect("Failed to read glob pattern")
        {
            match entry {
                Ok(path) => input_files.push(path.to_str().unwrap().to_string()),
                Err(e) => println!("{:?}", e),
            }
        }
        // To sort slices correctly taking into account the partition numbers.
        alphanumeric_sort::sort_str_slice(&mut input_files);
        log::warn!("Using {} files for {} table", input_files.len(), tpch_table);
        table_input.insert(tpch_table.to_string(), TableInput { input_files, scale });
    }
    table_input
}

pub fn save_df_to_csv(df: &mut DataFrame, file_path: &Path) {
    let mut output_file: File =
        File::create(&file_path).unwrap_or_else(|_| panic!("Failed to create {:?}", &file_path));
    CsvWriter::new(&mut output_file)
        .has_header(true)
        .finish(df)
        .unwrap();
}

fn _save_dfs_to_csv(dfs: &mut [DataFrame], dir_path: &Path) {
    // Prepare parent directory.
    std::fs::create_dir_all(dir_path).unwrap_or_else(|_| panic!("Failed to mkdir {:?}", dir_path));

    // Write each df one by one.
    for (idx, df) in dfs.iter_mut().enumerate() {
        let file_path = dir_path.join(format!("{}.csv", idx));
        save_df_to_csv(df, &file_path)
    }
    log::warn!(
        "Wrote {} dfs under {}",
        dfs.len(),
        dir_path.to_str().unwrap()
    )
}

#[derive(Serialize)]
pub struct MetaQueryResult<'a> {
    result_dir: &'a str,
    time_measures_ns: &'a [u128],
}

fn save_meta_result(
    result_dir: &Path,
    time_measures_ns: &[u128],
) -> std::result::Result<(), Box<dyn Error>> {
    let meta_json = serde_json::to_string(&MetaQueryResult {
        result_dir: result_dir.to_str().unwrap(),
        time_measures_ns,
    })?;
    let meta_path = result_dir.join("meta.json");
    let mut meta_file: File = File::create(&meta_path)?;
    meta_file.write_all(meta_json.as_bytes())?;
    log::warn!("Wrote meta query result to {}", meta_path.to_str().unwrap());
    Ok(())
}

pub fn run_query(
    _query_no: &str,
    query_service: &mut ExecutionService<DataFrame>,
    output_reader: &mut NodeReader<DataFrame>,
    results_dir: String,
    experiment: &str,
) -> usize {
    // Create results directory if doesn't exist
    let results_dir_path = Path::new(&results_dir);
    std::fs::create_dir_all(results_dir_path).unwrap_or_else(|_| panic!("Failed to mkdir {:?}", results_dir));

    let mut query_result_time_ns = Vec::new();
    let mut last_df = DataFrame::empty();
    let mut epoch = 0;

    let start_time = Instant::now();
    query_service.run();
    loop {
        let message = output_reader.read();
        if message.is_eof() {
            break;
        }
        let data = message.datablock().data();
        let duration = Instant::now() - start_time;
        if experiment == "accuracy" {
            // Save the result dataframe
            let file_path = results_dir_path.join(format!("{}.csv", epoch));
            save_df_to_csv(&mut data.clone(), &file_path);
            if epoch % 10 == 0 {
                log::warn!(
                    "Query Result {} Took: {:.2?}",
                    epoch + 1,
                    duration
                );
                log::warn!("Saved query result to {:?}", file_path);
            }
        }
        query_result_time_ns.push(duration.as_nanos());
        last_df = data.clone();
        epoch = epoch + 1;
    }
    query_service.join();
    let end_time = Instant::now();
    if epoch != 0 {
        log::warn!("Query Result");
        log::warn!("{:?}", last_df);
        log::warn!("Query Took: {:.2?}", end_time - start_time);
        let file_path = results_dir_path.join(format!("final.csv"));
        save_df_to_csv(&mut last_df, &file_path);
        log::warn!("Saved final query result to {:?}", file_path);
        save_meta_result(&results_dir_path, &query_result_time_ns).expect("Failed to write meta result");
    } else {
        log::error!("Empty Query Result");
    }
    epoch
}

pub fn check_file_format(file_names: &[String]) -> &str {
    // Check whether the first file name contains FILE_FORMAT_PARQUET.
    if file_names.is_empty() {
        FILE_FORMAT_DEFAULT
    } else {
        let file = file_names.get(0).unwrap();
        if file.contains(FILE_FORMAT_PARQUET) {
            FILE_FORMAT_PARQUET
        } else {
            FILE_FORMAT_CSV
        }
    }
}

pub fn build_reader_node(
    table: String,
    tableinput: &HashMap<String, TableInput>,
    table_columns: &HashMap<String, Vec<&str>>,
) -> ExecutionNode<polars::prelude::DataFrame> {
    // Get batch size and file names from tableinput tables;
    let raw_input_files = tableinput.get(&table as &str).unwrap().input_files.clone();
    let file_format = check_file_format(&raw_input_files);
    let scale = tableinput.get(&table as &str).unwrap().scale;
    let schema = tpch_schema(&table).unwrap();

    let mut projected_cols_index = None;
    let mut projected_cols_names = None;
    match table_columns.get(&table) {
        Some(columns) => {
            let mut cols_index = columns
                .iter()
                .map(|x| schema.index(x))
                .collect::<Vec<usize>>();
            cols_index.sort_unstable();
            let col_names = cols_index
                .iter()
                .map(|x| schema.get_column_from_index(*x).name)
                .collect::<Vec<String>>();
            projected_cols_index = Some(cols_index);
            projected_cols_names = Some(col_names);
        }
        None => {}
    }
    let input_files = df!("col" => &raw_input_files).unwrap();

    let reader = match file_format {
        FILE_FORMAT_CSV => CSVReaderBuilder::new()
            .delimiter('|')
            .has_headers(false)
            .column_names(projected_cols_names)
            .projected_cols(projected_cols_index)
            .build(),
        FILE_FORMAT_PARQUET => ParquetReaderBuilder::new()
            .column_names(projected_cols_names)
            .projected_cols(projected_cols_index)
            .build(),
        _ => {
            panic!("Invalid file format specified. Supported formats are tbl and parquet.")
        }
    };

    let mut metadata = MetaCell::Schema(schema).into_meta_map();
    *metadata
        .entry(DATABLOCK_TOTAL_RECORDS.to_string())
        .or_insert(MetaCell::Float(0.0)) =
        MetaCell::Float(total_number_of_records(&table, scale) as f64);

    let dblock = DataBlock::new(input_files, metadata);
    reader.write_to_self(0, DataMessage::from(dblock));
    reader.write_to_self(0, DataMessage::eof());
    reader
}

pub fn tpch_schema(table: &str) -> std::result::Result<wake::data::Schema, Box<dyn Error>> {
    let columns = match table {
        "lineitem" => vec![
            Column::from_key_field("l_orderkey".to_string(), wake::data::DataType::Integer),
            Column::from_field("l_partkey".to_string(), wake::data::DataType::Integer),
            Column::from_field("l_suppkey".to_string(), wake::data::DataType::Integer),
            Column::from_key_field("l_linenumber".to_string(), wake::data::DataType::Integer),
            Column::from_field("l_quantity".to_string(), wake::data::DataType::Integer),
            Column::from_field("l_extendedprice".to_string(), wake::data::DataType::Float),
            Column::from_field("l_discount".to_string(), wake::data::DataType::Float),
            Column::from_field("l_tax".to_string(), wake::data::DataType::Float),
            Column::from_field("l_returnflag".to_string(), wake::data::DataType::Text),
            Column::from_field("l_linestatus".to_string(), wake::data::DataType::Text),
            Column::from_field("l_shipdate".to_string(), wake::data::DataType::Text),
            Column::from_field("l_commitdate".to_string(), wake::data::DataType::Text),
            Column::from_field("l_receiptdate".to_string(), wake::data::DataType::Text),
            Column::from_field("l_shipinstruct".to_string(), wake::data::DataType::Text),
            Column::from_field("l_shipmode".to_string(), wake::data::DataType::Text),
            Column::from_field("l_comment".to_string(), wake::data::DataType::Text),
        ],
        "orders" => vec![
            Column::from_key_field("o_orderkey".to_string(), wake::data::DataType::Integer),
            Column::from_field("o_custkey".to_string(), wake::data::DataType::Integer),
            Column::from_field("o_orderstatus".to_string(), wake::data::DataType::Text),
            Column::from_field("o_totalprice".to_string(), wake::data::DataType::Float),
            Column::from_field("o_orderdate".to_string(), wake::data::DataType::Text),
            Column::from_field("o_orderpriority".to_string(), wake::data::DataType::Text),
            Column::from_field("o_clerk".to_string(), wake::data::DataType::Text),
            Column::from_field("o_shippriority".to_string(), wake::data::DataType::Integer),
            Column::from_field("o_comment".to_string(), wake::data::DataType::Text),
        ],
        "customer" => vec![
            Column::from_key_field("c_custkey".to_string(), wake::data::DataType::Integer),
            Column::from_field("c_name".to_string(), wake::data::DataType::Text),
            Column::from_field("c_address".to_string(), wake::data::DataType::Text),
            Column::from_field("c_nationkey".to_string(), wake::data::DataType::Integer),
            Column::from_field("c_phone".to_string(), wake::data::DataType::Text),
            Column::from_field("c_acctbal".to_string(), wake::data::DataType::Float),
            Column::from_field("c_mktsegment".to_string(), wake::data::DataType::Text),
            Column::from_field("c_comment".to_string(), wake::data::DataType::Text),
        ],
        "supplier" => vec![
            Column::from_key_field("s_suppkey".to_string(), wake::data::DataType::Integer),
            Column::from_field("s_name".to_string(), wake::data::DataType::Text),
            Column::from_field("s_address".to_string(), wake::data::DataType::Text),
            Column::from_field("s_nationkey".to_string(), wake::data::DataType::Integer),
            Column::from_field("s_phone".to_string(), wake::data::DataType::Text),
            Column::from_field("s_acctbal".to_string(), wake::data::DataType::Float),
            Column::from_field("s_comment".to_string(), wake::data::DataType::Text),
        ],
        "nation" => vec![
            Column::from_key_field("n_nationkey".to_string(), wake::data::DataType::Integer),
            Column::from_field("n_name".to_string(), wake::data::DataType::Text),
            Column::from_field("n_regionkey".to_string(), wake::data::DataType::Integer),
            Column::from_field("n_comment".to_string(), wake::data::DataType::Text),
        ],
        "region" => vec![
            Column::from_key_field("r_regionkey".to_string(), wake::data::DataType::Integer),
            Column::from_field("r_name".to_string(), wake::data::DataType::Text),
            Column::from_field("r_comment".to_string(), wake::data::DataType::Text),
        ],
        "part" => vec![
            Column::from_key_field("p_partkey".to_string(), wake::data::DataType::Integer),
            Column::from_field("p_name".to_string(), wake::data::DataType::Text),
            Column::from_field("p_mfgr".to_string(), wake::data::DataType::Text),
            Column::from_field("p_brand".to_string(), wake::data::DataType::Text),
            Column::from_field("p_type".to_string(), wake::data::DataType::Text),
            Column::from_field("p_size".to_string(), wake::data::DataType::Integer),
            Column::from_field("p_container".to_string(), wake::data::DataType::Text),
            Column::from_field("p_retailprice".to_string(), wake::data::DataType::Float),
            Column::from_field("p_comment".to_string(), wake::data::DataType::Text),
        ],
        "partsupp" => vec![
            Column::from_key_field("ps_partkey".to_string(), wake::data::DataType::Integer),
            Column::from_key_field("ps_suppkey".to_string(), wake::data::DataType::Integer),
            Column::from_field("ps_availqty".to_string(), wake::data::DataType::Integer),
            Column::from_field("ps_supplycost".to_string(), wake::data::DataType::Float),
            Column::from_field("ps_comment".to_string(), wake::data::DataType::Text),
        ],
        _ => vec![],
    };
    if columns.is_empty() {
        Err("Schema Not Defined".into())
    } else {
        Ok(wake::data::Schema::new(String::from(table), columns))
    }
}
