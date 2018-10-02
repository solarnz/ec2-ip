extern crate clap;
extern crate rusoto_core;
extern crate rusoto_ec2;
extern crate skim;

use clap::{App, Arg};
use rusoto_core::Region;
use rusoto_ec2::{DescribeInstancesRequest, Ec2, Ec2Client, Filter, Instance};
use skim::{Skim, SkimOptions};
use std::collections::HashMap;
use std::default::Default;
use std::io::Cursor;

fn get_instances(
    region_name: String,
    filters: Option<Vec<Filter>>,
) -> Result<Vec<Instance>, String> {
    let region: Region = match region_name.parse() {
        Ok(region) => region,
        Err(_err) => return Err("Invalid region name".to_string()),
    };

    let client = Ec2Client::new(region);
    let mut region_instances: Vec<Instance> = vec![];

    let mut input = DescribeInstancesRequest {
        dry_run: None,
        filters: filters,
        instance_ids: None,
        max_results: Some(1000),
        next_token: None,
    };

    let mut first = true;

    while first || input.next_token.is_some() {
        let result = match client.describe_instances(input.clone()).sync() {
            Ok(result) => result,
            Err(err) => return Err(err.to_string()),
        };

        if let Some(reservations) = result.reservations {
            for reservation in reservations {
                if let Some(instances) = reservation.instances {
                    for instance in instances {
                        region_instances.push(instance);
                    }
                }
            }
        }

        input.next_token = result.next_token;
        first = false;
    }

    return Ok(region_instances);
}

pub fn main() {
    let options = App::new("ec2-skim")
        .arg(
            Arg::with_name("region")
                .help("The region to search for instances in")
                .takes_value(true)
                .short("r")
                .long("region")
                .multiple(true)
                .required(true),
        )
        .arg(
            Arg::with_name("public_ip")
                .help("Returns the public ip of the selected instance")
                .long("public-ip")
                .takes_value(false),
        )
        .arg(
            Arg::with_name("filter")
                .help("EC2 filters to filter by")
                .short("f")
                .long("filter")
                .takes_value(true)
                .multiple(true),
        )
        .arg(
            Arg::with_name("display_tag")
                .help("Which tags to show in the list of instances")
                .short("d")
                .long("display-tag")
                .takes_value(true)
                .multiple(true),
        )
        .get_matches();

    let mut all_instances: HashMap<String, Instance> = HashMap::new();

    // We allow for multiple filters options. If this happens, we need to fetch the instances for
    // each set.
    let mut filter_groups: Vec<Vec<Filter>> = vec![];

    if let Some(all_filter_strings) = options.values_of("filter") {
        for filter_string in all_filter_strings {
            let filter_options: Vec<&str> = filter_string.split(";").collect();

            // We will always assume we're looking for currently running instances
            let mut filters: Vec<Filter> = vec![Filter {
                name: Some("instance-state-name".to_string()),
                values: Some(vec!["running".to_string()]),
            }];

            for filter in filter_options {
                let mut parts: Vec<String> = filter.split("=").map(String::from).collect();

                let name = Some(parts.remove(0));

                let values: Option<Vec<String>> = if parts.len() > 0 {
                    Some(parts[0].split(',').map(String::from).collect())
                } else {
                    None
                };

                filters.push(Filter {
                    name: name,
                    values: values,
                });
            }

            filter_groups.push(filters);
        }
    } else {
        let mut filters: Vec<Filter> = vec![Filter {
            name: Some("instance-state-name".to_string()),
            values: Some(vec!["running".to_string()]),
        }];
        filter_groups.push(filters);
    }

    if let Some(regions) = options.values_of("region") {
        for region in regions {
            for filters in filter_groups.clone() {
                let instances = match get_instances(region.to_string(), Some(filters.clone())) {
                    Ok(instances) => instances.clone(),
                    Err(err) => panic!(err),
                };

                for instance in instances {
                    if let Some(instance_id) = instance.clone().instance_id {
                        all_instances.insert(instance_id, instance);
                    }
                }
            }
        }
    }

    let mut skim_input = String::new();

    let instances = all_instances.values();

    let mut display_tags: Vec<String> = vec!["Name".to_string()];
    if let Some(extra_display_tags) = options.values_of("display_tag") {
        display_tags.extend(extra_display_tags.map(String::from));
    }

    for instance in instances.clone() {
        if let Some(ref instance_id) = instance.instance_id {
            skim_input.push_str(format!("{:19}: ", instance_id).as_str());
        }

        if let Some(tags) = instance.clone().tags {
            let mut tag_map: HashMap<String, String> = HashMap::new();

            for tag in tags {
                if let Some(key) = tag.key {
                    if let Some(value) = tag.value {
                        tag_map.insert(key, value);
                    }
                }
            }

            for display_tag in display_tags.clone() {
                if let Some(value) = tag_map.get(&display_tag) {
                    skim_input.push_str(format!("{}={} ", &display_tag, value).as_str());
                }
            }
        }

        skim_input.push_str("\n");
    }

    let skim_options: SkimOptions = SkimOptions::default();

    let selected_items = Skim::run_with(&skim_options, Some(Box::new(Cursor::new(skim_input))))
        .map(|out| out.selected_items)
        .unwrap_or_else(|| Vec::new());

    for item in selected_items.iter() {
        if let Some(instance) = instances.clone().nth(item.get_index()) {
            if options.is_present("public_ip") {
                print!("{}", instance.clone().public_ip_address.unwrap());
            } else {
                print!("{}", instance.clone().private_ip_address.unwrap());
            }
        }
    }
}
