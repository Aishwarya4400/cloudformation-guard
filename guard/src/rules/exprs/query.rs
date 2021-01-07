//
// This file implements query semantics on structure Value types
//

use crate::rules::values::*;
use crate::errors::{Error, ErrorKind};
use std::collections::HashMap;
use super::*;
use super::helper::*;

use std::fmt::Formatter;

#[derive(Clone, Debug)]
pub(crate) struct QueryResolver {}

impl Resolver for QueryResolver {
    fn resolve_query<'r>(&self,
                         query: &[QueryPart<'_>],
                         value: &'r Value,
                         variables: &Scope<'_>,
                         path: Path,
                         eval: &EvalContext<'_>) -> Result<ResolvedValues<'r>, Error> {
        let mut results = ResolvedValues::new();
        let mut value_ref = value;
        let mut path_ref = path;

        for (index, query_part) in query.iter().enumerate() {
            if query_part.is_variable() {
                return Err(Error::new(ErrorKind::IncompatibleError(
                    "Do not support variable interpolation inside a query".to_string()
                )))
            }
            match query_part {
                QueryPart::Key(key) => {
                    //
                    // Support old format
                    //
                    match key.parse::<i32>() {
                        Ok(idx) => {
                            value_ref = retrieve_index(idx, value_ref, &path_ref)?;
                            path_ref = path_ref.append(idx.to_string());
                        },
                        Err(_) => {
                            value_ref = retrieve_key(key, value_ref, &path_ref)?;
                            path_ref = path_ref.append_str(key);
                        }
                    }
                },

                QueryPart::Index(idx) => {
                    value_ref = retrieve_index(*idx, value_ref, &path_ref)?;
                    path_ref = path_ref.append((*idx).to_string());
                },

//                QueryPart::Index(key, idx) => {
//                    value_ref = retrieve_key(key, value_ref, &path_ref)?;
//                    path_ref = path_ref.append_str(key);
//                    value_ref = retrieve_index(*idx, value_ref, &path_ref)?;
//                    path_ref = path_ref.append((*idx).to_string());
//                },

                QueryPart::AllKeys => {
                    //
                    // Support old format
                    //
                    match match_list(value_ref, &path_ref) {
                        Err(_) =>
                            return self.handle_map(match_map(value_ref, &path_ref)?,
                                              index, path_ref, query, variables, eval),

                        Ok(array) =>
                            return self.handle_array(array, index, path_ref, query, variables, eval),
                    }
                },

                QueryPart::AllIndices => {
                    return self.handle_array(match_list(value_ref, &path_ref)?,
                                        index, path_ref, query, variables, eval)
                },

//                QueryPart::AllIndices(key) => {
//                    value_ref = retrieve_key(key, value_ref, &path_ref)?;
//                    path_ref = path_ref.append_str(key);
//                    return self.handle_array(match_list(value_ref, &path_ref)?,
//                                        index, path_ref, query, variables, eval)
//                },

//                QueryPart::Filter(key, criteria) => {
//                    let mut collected = Vec::new();
//                    if key == "*" {
//                        let map = match_map(value_ref, &path_ref)?;
//                        for (k, v) in map {
//                            let sub_path = path_ref.clone().append_str(k.as_str());
//                            if self.select(criteria, v, variables, &path_ref, eval)? {
//                                collected.push((sub_path, v));
//                            }
//                        }
//                    } else {
//                        value_ref = retrieve_key(key, value_ref, &path_ref)?;
//                        path_ref = path_ref.append_str(key);
//                        let list = match_list(value_ref, &path_ref)?;
//                        for (idx, each) in list.iter().enumerate() {
//                            if self.select(criteria, each, variables, &path_ref, eval)? {
//                                collected.push((path_ref.clone().append(idx.to_string()), each));
//                            }
//                        }
//                    }
//
//                    for (p, v) in collected {
//                        let sub_query = self.resolve_query(
//                             &query[index + 1..], v, variables, p, eval)?;
//                        results.extend(sub_query);
//                    }
//                    return Ok(results)
//                }

                _ => unimplemented!()
            }
        }

        results.insert(path_ref, value_ref);
        Ok(results)
    }
}

impl QueryResolver {

    fn select(&self,
              criteria: &Conjunctions<GuardClause<'_>>,
              value: &Value,
              scope: &Scope<'_>,
              path: &Path,
              eval: &EvalContext<'_>) -> Result<bool, Error> {
        Ok(
            match criteria.evaluate(self, scope, value, path.clone(), eval)? {
                EvalStatus::Unary(Status::PASS) => true,
                EvalStatus::Comparison(EvalResult{ status: Status::PASS, from, to}) => true,
                _ => false
            }
        )
    }

    fn handle_array<'loc>(&self,
                          array: &'loc Vec<Value>,
                          index: usize,
                          path: Path,
                          query: &[QueryPart<'_>],
                          scope: &Scope<'_>,
                          eval: &EvalContext<'_>) -> Result<ResolvedValues<'loc>, Error> {
        let mut results = ResolvedValues::new();
        for (each_idx, each_value) in array.iter().enumerate() {
            let sub_path = path.clone().append(each_idx.to_string());
            let sub_query = self.resolve_query(
                 &query[index+1..], each_value, scope, sub_path, eval)?;
            results.extend(sub_query);
        }
        Ok(results)
    }

    fn handle_map<'loc>(&self,
                        map: &'loc indexmap::IndexMap<String, Value>,
                        index: usize,
                        path: Path,
                        query: &[QueryPart<'_>],
                        scope: &Scope<'_>,
                        eval: &EvalContext<'_>) -> Result<ResolvedValues<'loc>, Error> {
        let mut results = ResolvedValues::new();
        for (key, index_value) in map {
            let sub_path = path.clone().append_str(key);
            let sub_query = self.resolve_query(
                &query[index+1..], index_value, scope, sub_path, eval)?;
            results.extend(sub_query);
        }
        Ok(results)
    }


}

#[cfg(test)]
mod tests {

    use super::*;
    use std::collections::HashSet;
    use crate::rules::parser2::{parse_value, from_str2, access};
    use std::fs::File;
    use crate::commands::files::{get_files, read_file_content};

    struct Eval{}
    impl Evaluate for Eval {
        type Item = EvalStatus;

        fn evaluate(&self,
                    resolver: &dyn Resolver,
                    scope: &Scope<'_>,
                    context: &Value,
                    path: Path,
                    eval_context: &EvalContext<'_>) -> Result<Self::Item, Error> {
            unimplemented!()
        }
    }

    fn create_from_json(path: &str) -> Result<Value, Error> {
        let file = File::open(path)?;
        let context = read_file_content(file)?;
        Ok(parse_value(from_str2(&context))?.1)
    }

    #[test]
    fn test_resolve_query() -> Result<(), Error> {
        let root = create_from_json("assets/cfn-template.json")?;
        //let mut cache = EvalContext::new(&root);
        let scope = Scope::new();
        let path = Path::new(&["/"]);
        let rules = RulesFile {
            guard_rules: vec![],
            assignments: vec![]
        };
        let eval_cxt = EvalContext::new(root, &rules);
        let map = match_map(&eval_cxt.root, &path)?;
        let resolver = QueryResolver{};
        let evaluate = Eval{};

        //
        // Test base empty query
        //
        let values = resolver.resolve_query(
            &[], &eval_cxt.root, &scope, Path::new(&["/"]), &eval_cxt)?;
        assert_eq!(values.len(), 1);
        assert_eq!(values.get(&Path::new(&["/"])), Some(&&eval_cxt.root));

        //
        // Path = Resources
        //
        let query = AccessQuery::from([
            QueryPart::Key(String::from("Resources"))
        ]);
        let values =
            resolver.resolve_query(
                &query, &eval_cxt.root, &scope, Path::new(&["/"]), &eval_cxt)?;
        assert_eq!(values.len(), 1);
        assert_eq!(Some(values[&Path::new(&["/", "Resources"])]), map.get("Resources"));
        let from_root = map.get("Resources");
        assert!(values[&Path::new(&["/", "Resources"])] == map.get("Resources").unwrap());

        let resources_root = match_map(from_root.unwrap(), &path)?;
        //
        // Path = Resources.*
        //
        let query = AccessQuery::from([
            QueryPart::Key(String::from("Resources")),
            QueryPart::AllKeys
        ]);
        let values =
            resolver.resolve_query(
                &query, &eval_cxt.root, &scope, Path::new(&["/"]), &eval_cxt)?;

        assert_eq!(resources_root.len(), values.len());

        let paths = resources_root.keys().map(|s: &String| Path::new(&["/", "Resources", s.as_str()]))
            .collect::<Vec<Path>>();
        let paths_values = values.iter().map(|(path, _value)| path.clone())
            .collect::<Vec<Path>>();
        assert_eq!(paths_values, paths);

        //
        // Path = Resources.*.Type
        //
        let query = AccessQuery::from([
            QueryPart::Key(String::from("Resources")),
            QueryPart::AllKeys,
            QueryPart::Key(String::from("Type")),
        ]);
        let values =
            resolver.resolve_query(
                &query, &eval_cxt.root, &scope, Path::new(&["/"]), &eval_cxt)?;

        assert_eq!(resources_root.len(), values.len());
        let paths = resources_root.keys().map(|s: &String| Path::new(&["/", "Resources", s.as_str(), "Type"]))
            .collect::<Vec<Path>>();
        let paths_values = values.iter().map(|(path, _value)| path.clone())
            .collect::<Vec<Path>>();
        assert_eq!(paths_values, paths);

        let types = resources_root.values().map(|v|
            if let Value::Map(m) = v {
            m.get("Type").unwrap()
        } else { unreachable!() }).collect::<Vec<&Value>>();

        let types_values = values.iter().map(|(_path, value)| *value).collect::<Vec<&Value>>();
        assert_eq!(types_values, types);

//        let mut scope = Scope::new();
//        let value_literals = vec![
//            Value::String(String::from("Type")),
//            Value::String(String::from("Properties"))
//        ];
//        let value_resolutions = vec![
//            (path.clone(), &value_literals[0]),
//            (path.clone().append_str("/"), &value_literals[1]),
//        ];
//        let resolutions = value_resolutions.into_iter().collect::<ResolvedValues>();
//
//        scope.add_variable_resolution("interested", resolutions);
//
//        //
//        // Path = Resources.*.%interested
//        //
//        let query = AccessQuery::from([
//            QueryPart::Key(String::from("Resources")),
//            QueryPart::AllKeys,
//            QueryPart::Key(String::from("%interested")),
//        ]);
//        let values =
//            resolver.resolve_query(
//                &query, &eval_cxt.root, &scope, Path::new(&["/"]), &eval_cxt)?;
//
//        assert_eq!(resources_root.len() * 2, values.len()); // one for types and the other for properties
//        let paths = resources_root.keys().map(|s: &String| Path::new(&["/", "Resources", s.as_str(), "Type"]))
//            .collect::<Vec<Path>>();
//        let paths_properties = resources_root.keys().map(|s: &String| Path::new(&["/", "Resources", s.as_str(), "Properties"]))
//            .collect::<Vec<Path>>();
//
//        let mut overall: Vec<Path> = Vec::with_capacity(paths.len() * 2);
//        for (first, second) in paths.iter().zip(paths_properties.iter()) {
//            overall.push(first.clone());
//            overall.push(second.clone());
//        }
//
//        let paths = overall;
//        let paths_values = values.iter().map(|(path, _value)| path.clone())
//            .collect::<Vec<Path>>();
//        assert_eq!(paths_values, paths);
//
//        let types = resources_root.values().map(|v|
//            if let Value::Map(m) = v {
//                m.get("Type").unwrap()
//            } else { unreachable!() }).collect::<Vec<&Value>>();
//        let properties = resources_root.values().map(|v|
//            if let Value::Map(m) = v {
//                m.get("Properties").unwrap()
//            } else { unreachable!() }).collect::<Vec<&Value>>();
//
//        let mut combined: Vec<&Value> = Vec::with_capacity(types.len() * 2);
//        for (first, second) in types.iter().zip(properties.iter()) {
//            combined.push(first);
//            combined.push(second);
//        }
//
//        let types_values = values.iter().map(|(_path, value)| *value).collect::<Vec<&Value>>();
//        assert_eq!(types_values, combined);
//

        Ok(())
    }

    #[test]
    fn test_opa_sample() -> Result<(), Error> {
        let root = create_from_json("assets/opa-sample.json")?;
        let mut scope = Scope::new();
        let resolver = QueryResolver{};
        let rules = RulesFile {
            guard_rules: vec![],
            assignments: vec![]
        };
        let eval = EvalContext::new(root, &rules);

        let evaluate = Eval{};
        let protocols = AccessQuery::from([
            QueryPart::Key(String::from("servers")),
            QueryPart::AllIndices,
            QueryPart::Key(String::from("protocols")),
            QueryPart::AllIndices
        ]);

        let root_path = Path::new(&[""]);
        let servers = match_map(&eval.root, &root_path)?;
        let mut protocols_flattened = Vec::with_capacity(servers.len());
        let servers = servers.get("servers").unwrap();
        let servers = match_list(servers, &root_path)?;
        for (serv_idx, server) in servers.iter().enumerate() {
            let each = match_map(server, &root_path)?;
            for each in match_list(each.get("protocols").unwrap(), &root_path) {
                for proto in each.iter().enumerate() {
                    protocols_flattened.push((serv_idx, proto));
                }
            }
        }

        let resolved = resolver.resolve_query(
            &protocols, &eval.root, &scope, Path::new(&["/"]), &eval)?;
        let mut expected = ResolvedValues::new();
        for (serv_idx, (prot_idx, val)) in protocols_flattened {
            let idx_string = prot_idx.to_string();
            let serv_idx_string = serv_idx.to_string();
            expected.insert(Path::new(&["/", "servers", &serv_idx_string, "protocols", &idx_string]), val);
        }

        println!("Expected {:?}, Actual {:?}", expected, resolved);
        assert_eq!(expected, resolved);

        let query = AccessQuery::from([
            QueryPart::Key(String::from("servers")),
            QueryPart::Index(0),
            QueryPart::Key(String::from("protocols")),
            QueryPart::Index(0),
        ]);
        let resolved = resolver.resolve_query(
            &query, &eval.root, &scope, Path::new(&["/"]), &eval)?;
        let mut expected = ResolvedValues::new();
        let first = servers.get(0).unwrap();
        let first = match_map(first, &root_path)?;
        let protocol = match_list(first.get("protocols").unwrap(), &root_path)?.get(0).unwrap();
        expected.insert(Path::new(&["/", "servers", "0", "protocols", "0"]), protocol);
        assert_eq!(expected, resolved);

        Ok(())
    }

    const IAM_TEMPLATE: &str = r#"
    { "Policy":
      {
        "Version": "2012-10-17",
        "Statement": [
            {
                "Sid": "PrincipalPutObjectIfIpAddress",
                "Effect": "Allow",
                "Action": "s3:PutObject",
                "Resource": "arn:aws:s3:::my-service-bucket/*",
                "Condition": {
                    "Bool": {"aws:ViaAWSService": "false"},
                    "StringEquals": {"aws:SourceVpc": "vpc-12243sc"}
                }
            },
            {
                "Sid": "ServicePutObject",
                "Effect": "Allow",
                "Action": "s3:PutObject",
                "Resource": "arn:aws:s3:::my-service-bucket/*",
                "Condition": {
                    "Bool": {"aws:ViaAWSService": "true"}
                }
            }
        ]
      }
   }"#;

    #[test]
    fn test_iam_query() -> Result<(), Error> {
        let iam_policy = parse_value(from_str2(IAM_TEMPLATE))?.1;

        let mut scope = Scope::new();
        let resolver = QueryResolver{};
        let rules = RulesFile {
            guard_rules: vec![],
            assignments: vec![]
        };
        let eval = EvalContext::new(iam_policy, &rules);

        let query = access(from_str2("Policy.Statement[*].Condition.*[ KEYS == /aws:[sS]ource(Vpc|VPC|Vpce|VPCE)/ ]"))?.1;
        let selected = resolver.resolve_query(&query, &eval.root, &scope, Path::new(&["/"]), &eval)?;
        assert_eq!(selected.is_empty(), false);
        assert_eq!(selected.len(), 1);
        let path = "Policy.Statement.0.Condition.StringEquals";
        let expected = eval.root.traverse(path)?;
        let real_path = Path::new(&path.split(".").collect::<Vec<&str>>());
        let real_path = real_path.prepend_str("/");
        let matched = match selected.get(&real_path) {
            Some(v) => *v,
            None => unreachable!()
        };
        println!("expected = {:?}, expected_addr = {:p}, matched = {:?}, matched_addr = {:p}", expected, expected, matched, matched);
        assert_eq!(expected, matched);
        assert_eq!(std::ptr::eq(expected, matched), true);

        let query = access(from_str2("Policy.Statement[*].Condition.*[ KEYS == /aws:ViaAWS/ ]"))?.1;
        let selected = resolver.resolve_query(&query, &eval.root, &scope, Path::new(&["/"]), &eval)?;
        assert_eq!(selected.is_empty(), false);
        assert_eq!(selected.len(), 2);
        let path = [
            "Policy.Statement.0.Condition.Bool",
            "Policy.Statement.1.Condition.Bool",
        ];

        for each_path in &path {
            let expected = eval.root.traverse(*each_path)?;
            let real_path = Path::new(&(*each_path).split(".").collect::<Vec<&str>>());
            let real_path = real_path.prepend_str("/");
            let matched = match selected.get(&real_path) {
                Some(v) => *v,
                None => unreachable!()
            };
            println!("expected = {:?}, expected_addr = {:p}, matched = {:?}, matched_addr = {:p}", expected, expected, matched, matched);
            assert_eq!(expected, matched);
            assert_eq!(std::ptr::eq(expected, matched), true);
        }

        let selection_query = r#"Policy.Statement[ Condition EXISTS
                                                         Condition.Bool.'aws:ViaAWSService' EXISTS ]"#;
        let query = access(from_str2(selection_query))?.1;
        let selected = resolver.resolve_query(&query, &eval.root, &scope, Path::new(&["/"]), &eval)?;
        println!("Selected = {:?}", selected);
        let path = [
            "Policy.Statement.0",
            "Policy.Statement.1"
        ];

        for each_path in &path {
            let expected = eval.root.traverse(*each_path)?;
            let real_path = Path::new(&(*each_path).split(".").collect::<Vec<&str>>());
            let real_path = real_path.prepend_str("/");
            let matched = match selected.get(&real_path) {
                Some(v) => *v,
                None => unreachable!()
            };
            println!("expected = {:?}, expected_addr = {:p}, matched = {:?}, matched_addr = {:p}", expected, expected, matched, matched);
            assert_eq!(expected, matched);
            assert_eq!(std::ptr::eq(expected, matched), true);
        }


        Ok(())
    }


}
