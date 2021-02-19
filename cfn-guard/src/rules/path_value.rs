//
// Std Libraries
//
use std::fmt::Formatter;
use std::convert::TryFrom;

//
// crate level
//

//
// Local mod
//
use super::values::*;
use super::errors::{Error, ErrorKind};
use super::exprs::{QueryPart, SliceDisplay};
use super::{EvaluationContext, Evaluate, Status};
use std::cmp::Ordering;
use crate::rules::evaluate::AutoReport;
use crate::rules::EvaluationType;
use serde::{Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
pub(crate) struct Path(pub(crate) String);

impl std::fmt::Display for Path {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl Path {
    pub fn root() -> Self {
        Path("".to_string())
    }
}

impl TryFrom<&str> for Path {
    type Error = Error;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Ok(Path(value.to_string()))
    }
}

impl TryFrom<&[&str]> for Path {
    type Error = Error;

    fn try_from(value: &[&str]) -> Result<Self, Self::Error> {
        Ok(Path(value.iter().map(|s| (*s).to_string())
            .fold(String::from(""), |mut acc, part| {
                if acc.is_empty() {
                    acc.push_str(part.as_str());
                } else {
                    acc.push('/'); acc.push_str(part.as_str());
                }
                acc
            })))
    }
}

impl TryFrom<&[String]> for Path {
    type Error = Error;

    fn try_from(value: &[String]) -> Result<Self, Self::Error> {
        let vec = value.iter().map(String::as_str).collect::<Vec<&str>>();
        Path::try_from(vec.as_slice())
    }
}

impl Path {
    pub(crate) fn extend_str(&self, part: &str) -> Path {
        let mut copy = self.0.clone();
        copy.push('/');
        copy.push_str(part);
        Path(copy)
    }

    pub(crate) fn extend_string(&self, part: &String) -> Path {
        self.extend_str(part.as_str())
    }

    pub(crate) fn extend_usize(&self, part: usize) -> Path {
        let as_str = part.to_string();
        self.extend_string(&as_str)
    }

    pub(crate) fn drop_last(&mut self) -> &mut Self {
        let removed = match self.0.rfind('/') {
            Some(idx) => self.0.as_str()[0..idx].to_string(),
            None => return self
        };
        self.0 = removed;
        self
    }

    pub(crate) fn extend_with_value(&self, part: &Value) -> Result<Path, Error> {
        match part {
            Value::String(s) => Ok(self.extend_string(s)),
            _ => Err(Error::new(ErrorKind::IncompatibleError(
                format!("Value type is not String, Value = {:?}", part)
            )))
        }
    }
}

#[derive(PartialEq, Debug, Clone, Serialize)]
pub(crate) struct MapValue {
    keys: Vec<PathAwareValue>,
    values: indexmap::IndexMap<String, PathAwareValue>,
}


#[derive(Debug, Clone, Serialize)]
pub(crate) enum PathAwareValue {
    Null(Path),
    String((Path, String)),
    Regex((Path, String)),
    Bool((Path, bool)),
    Int((Path, i64)),
    Float((Path, f64)),
    Char((Path, char)),
    List((Path, Vec<PathAwareValue>)),
    Map((Path, MapValue)),
    RangeInt((Path, RangeType<i64>)),
    RangeFloat((Path, RangeType<f64>)),
    RangeChar((Path, RangeType<char>)),
}

impl PartialEq for PathAwareValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (PathAwareValue::Map((_, map)), PathAwareValue::Map((_, map2))) => map == map2,

            (PathAwareValue::List((_, list)), PathAwareValue::List((_, list2))) => list == list2,

            (rest, rest2) => match compare_values(rest, rest2) {
                    Ok(ordering) => match ordering {
                        Ordering::Equal => true,
                        _ => false
                    },
                    Err(_) => false
                }
        }
    }
}

impl TryFrom<&str> for PathAwareValue {
    type Error = Error;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let value = Value::try_from(value)?;
        PathAwareValue::try_from((&value, Path::try_from("")?))
    }
}

impl TryFrom<(&str, Path)> for PathAwareValue {
    type Error = Error;

    fn try_from(value: (&str, Path)) -> Result<Self, Self::Error> {
        let parsed = Value::try_from(value.0)?;
        PathAwareValue::try_from((&parsed, value.1))
    }
}

impl TryFrom<(&serde_json::Value, Path)> for PathAwareValue {
    type Error = Error;

    fn try_from(incoming: (&serde_json::Value, Path)) -> Result<Self, Self::Error> {
        let root = incoming.0;
        let path = incoming.1;
        let value = Value::try_from(root)?;
        PathAwareValue::try_from((&value, path))
    }
}

impl TryFrom<Value> for PathAwareValue {
    type Error = Error;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        PathAwareValue::try_from((&value, Path::root()))
    }
}

impl TryFrom<serde_json::Value> for PathAwareValue {
    type Error = Error;

    fn try_from(value: serde_json::Value) -> Result<Self, Self::Error> {
        PathAwareValue::try_from((&value, Path::root()))
    }
}

impl TryFrom<(&Value, Path)> for PathAwareValue {
    type Error = Error;

    fn try_from(incoming: (&Value, Path)) -> Result<Self, Self::Error> {
        let root = incoming.0;
        let path = incoming.1;

        match root {
            Value::String(s) => Ok(PathAwareValue::String((path, s.to_owned()))),
            Value::Int(num) => Ok(PathAwareValue::Int((path, *num))),
            Value::Float(flt) => Ok(PathAwareValue::Float((path, *flt))),
            Value::Regex(s) => Ok(PathAwareValue::Regex((path, s.clone()))),
            Value::Char(c) => Ok(PathAwareValue::Char((path, *c))),
            Value::RangeChar(r) => Ok(PathAwareValue::RangeChar((path, r.clone()))),
            Value::RangeInt(r) => Ok(PathAwareValue::RangeInt((path, r.clone()))),
            Value::RangeFloat(r) => Ok(PathAwareValue::RangeFloat((path, r.clone()))),
            Value::Bool(b) => Ok(PathAwareValue::Bool((path, *b))),
            Value::Null => Ok(PathAwareValue::Null(path)),
            Value::List(v) => {
                let mut result: Vec<PathAwareValue> = Vec::with_capacity(v.len());
                for (idx, each) in v.iter().enumerate() {
                    let sub_path = path.extend_usize(idx);
                    let value = PathAwareValue::try_from((each, sub_path.clone()))?;
                    result.push(value);
                }
                Ok(PathAwareValue::List((path, result)))
            },

            Value::Map(map) => {
                let mut keys = Vec::with_capacity(map.len());
                let mut values = indexmap::IndexMap::with_capacity(map.len());
                for each_key in map.keys() {
                    let sub_path = path.extend_string(each_key);
                    let value = PathAwareValue::String((sub_path, each_key.to_string()));
                    keys.push(value);
                }

                for (each_key, each_value) in map {
                    let sub_path = path.extend_string(each_key);
                    let value = PathAwareValue::try_from((each_value, sub_path))?;
                    values.insert(each_key.to_owned(), value);
                }
                Ok(PathAwareValue::Map((path, MapValue{keys, values})))
            }
        }
    }
}

pub(crate) trait QueryResolver {
    fn select(&self, all: bool, query: &[QueryPart<'_>], eval: &dyn EvaluationContext) -> Result<Vec<&PathAwareValue>, Error>;
}


impl QueryResolver for PathAwareValue {
    fn select(&self, all: bool, query: &[QueryPart<'_>], resolver: &dyn EvaluationContext) -> Result<Vec<&PathAwareValue>, Error> {
        if query.is_empty() {
            return Ok(vec![self])
        }

        match &query[0] {
            QueryPart::Key(key) => {
                match key.parse::<i32>() {
                    Ok(index) => {
                        match self {
                            PathAwareValue::List((_, list)) =>
                                PathAwareValue::retrieve_index(index, list, query)?.select(all, &query[1..], resolver),

                            _ => Err(Error::new(ErrorKind::IncompatibleError(
                                format!("Attempting to retrieve array index at Path = {}, Type was not an array {}, Remaining Query = {}",
                                        self.self_value().0, self.type_info(), SliceDisplay(query))
                            )))
                        }
                    },

                    Err(_) => match self {
                        PathAwareValue::Map((path, map)) => {
                            if let Some(next) = map.values.get(key) {
                                next.select(all, &query[1..], resolver)
                            } else {
                                Err(Error::new(
                                    ErrorKind::RetrievalError(
                                        format!("Could not locate key = {} inside object/map = {:?}, Path = {}, remaining query = {}",
                                                key, self, path, SliceDisplay(query))
                                    )))
                            }
                        },

                        _ => Err(Error::new(ErrorKind::IncompatibleError(
                            format!("Attempting to retrieve key/index at Path = {}, Type was not an object/map/array {}, Remaining Query = {}",
                                    self.self_value().0, self.type_info(), SliceDisplay(query))
                        )))
                    }
                }
            },

            QueryPart::MapKeys => {
                match self {
                    PathAwareValue::Map((_path, map)) => {
                        PathAwareValue::accumulate(all, &query[1..], &map.keys, resolver)
                    },

                    _ => Err(Error::new(ErrorKind::IncompatibleError(
                        format!("Attempting to retrieve KEYS from for type that is isn't a object/map {}(Pointer={})", self.type_info(), self.self_value().0)
                    )))
                }
            },

            QueryPart::Index(array_idx) => {
                match self {
                    PathAwareValue::List((_path, vec)) => {
                        PathAwareValue::retrieve_index(*array_idx, vec, query)?.select(all, &query[1..], resolver)
                    },

                    _ => Err(Error::new(ErrorKind::IncompatibleError(
                        format!("Attempting to retrieve array index at Path = {}, Type was not an array {}, Remaining Query = {}",
                                self.self_value().0, self.type_info(), SliceDisplay(query))
                    )))
                }
            },

            QueryPart::AllIndices => {
                match self {
                    PathAwareValue::List((_path, elements)) => {
                        PathAwareValue::accumulate(all, &query[1..], elements, resolver)
                    },

                    _ => Err(Error::new(ErrorKind::IncompatibleError(
                        format!("Attempting to retrieve ALL INDICES from for type that is isn't an array {}(Pointer={})", self.type_info(), self.self_value().0)
                    )))
                }
            }

            QueryPart::AllValues => {
                match self {
                    //
                    // Supporting old format
                    //
                    PathAwareValue::List((_path, elements)) => {
                        PathAwareValue::accumulate(all, &query[1..], elements, resolver)
                    },

                    PathAwareValue::Map((_path, map)) => {
                        let values: Vec<&PathAwareValue> = map.values.values().collect();
                        let mut resolved = Vec::with_capacity(values.len());
                        for each in values {
                            match each.select(all, &query[1..], resolver) {
                                Ok(result) => {
                                    resolved.extend(result);
                                },

                                Err(Error(ErrorKind::RetrievalError(e))) => {
                                    if all {
                                        return Err(Error::new(ErrorKind::RetrievalError(e)));
                                    }
                                },

                                Err(e) => return Err(e)
                            }
                        }
                        Ok(resolved)
                    },

                    _ => Err(Error::new(ErrorKind::IncompatibleError(
                        format!("Attempting to retrieve ALL VALUES from for type that is isn't a object/map/list {} (Pointer={})", self.type_info(), self.self_value().0)
                    )))
                }
            }

            QueryPart::Filter(conjunctions) => {
                match self {
                    PathAwareValue::List((path, vec)) => {
                        let mut selected = Vec::with_capacity(vec.len());
                        let context = format!("Path={},Type=Array", path);
                        for each in vec {
                            let mut filter = AutoReport::new(EvaluationType::Filter, resolver, &context);
                            match conjunctions.evaluate(each, resolver)? {
                                Status::PASS => {
                                    filter.status(Status::PASS);
                                    let index: usize = if query.len() > 1 {
                                        match &query[1] {
                                            QueryPart::AllIndices => 2,
                                            _ => 1
                                        }
                                    } else { 1 };
                                    selected.extend(each.select(all, &query[index..], resolver)?);
                                },
                                rest => { filter.status(rest); }
                            }
                        }
                        Ok(selected)
                    },

                    PathAwareValue::Map((path, _map)) => {
                        let context = format!("Path={},Type=MapElement", path);
                        let mut filter = AutoReport::new(EvaluationType::Filter, resolver, &context);
                        match conjunctions.evaluate(self, resolver)? {
                            Status::PASS => {
                                filter.status(Status::PASS);
                                self.select(all, &query[1..], resolver)
                            },
                            rest => {
                                filter.status(rest);
                                Ok(vec![])
                            }
                        }
                    }

                    _ => Err(Error::new(ErrorKind::IncompatibleError(
                        format!("Attempting to filter at Path = {}, Type was not an array/selected query {}, Remaining Query = {}",
                                self.self_value().0, self.type_info(), SliceDisplay(query))
                    )))
                }
            },
        }
    }
}

impl PathAwareValue {

    pub(crate) fn is_list(&self) -> bool {
        match self {
            PathAwareValue::List((_, _)) => true,
            _ => false,
        }
    }

    pub(crate) fn is_map(&self) -> bool {
        match self {
            PathAwareValue::Map((_, _)) => true,
            _ => false
        }
    }

    pub(crate) fn is_scalar(&self) -> bool {
        !self.is_list() || !self.is_map()
    }

    pub(crate) fn self_path(&self) -> &Path {
        self.self_value().0
    }

    pub(crate) fn self_value(&self) -> (&Path, &PathAwareValue) {
        match self {
            PathAwareValue::Null(path)              => (path, self),
            PathAwareValue::String((path, _))       => (path, self),
            PathAwareValue::Regex((path, _))        => (path, self),
            PathAwareValue::Bool((path, _))         => (path, self),
            PathAwareValue::Int((path, _))          => (path, self),
            PathAwareValue::Float((path, _))        => (path, self),
            PathAwareValue::Char((path, _))         => (path, self),
            PathAwareValue::List((path, _))         => (path, self),
            PathAwareValue::Map((path, _))          => (path, self),
            PathAwareValue::RangeInt((path, _))     => (path, self),
            PathAwareValue::RangeFloat((path, _))   => (path, self),
            PathAwareValue::RangeChar((path, _))    => (path, self),
        }
    }

    pub(crate) fn type_info(&self) -> &'static str {
        match self {
            PathAwareValue::Null(_path)              => "null",
            PathAwareValue::String((_path, _))       => "String",
            PathAwareValue::Regex((_path, _))        => "Regex",
            PathAwareValue::Bool((_path, _))         => "bool",
            PathAwareValue::Int((_path, _))          => "int",
            PathAwareValue::Float((_path, _))        => "float",
            PathAwareValue::Char((_path, _))         => "char",
            PathAwareValue::List((_path, _))         => "array",
            PathAwareValue::Map((_path, _))          => "map",
            PathAwareValue::RangeInt((_path, _))     => "range(int, int)",
            PathAwareValue::RangeFloat((_path, _))   => "range(float, float)",
            PathAwareValue::RangeChar((_path, _))    => "range(char, char)",
        }
    }

    pub(crate) fn retrieve_index<'v>(index: i32, list: &'v Vec<PathAwareValue>, query: &[QueryPart<'_>]) -> Result<&'v PathAwareValue, Error> {
        let check = if index >= 0 { index } else { -index } as usize;
        if check < list.len() {
            Ok(&list[check])
        } else {
            Err(Error::new(
                ErrorKind::RetrievalError(
                    format!("Array Index out of bounds on index = {} inside Array = {:?}, remaining query = {}",
                            index, list, SliceDisplay(query))
                )))
        }

    }

    pub(crate) fn accumulate<'v>(all: bool, query: &[QueryPart<'_>], elements: &'v Vec<PathAwareValue>, resolver: &dyn EvaluationContext) -> Result<Vec<&'v PathAwareValue>, Error>{
        let mut accumulated = Vec::with_capacity(elements.len());
        for each in elements {
            if !query.is_empty() {
                match each.select(all, &query[1..], resolver) {
                    Ok(result) => {
                        accumulated.extend(result);
                    },

                    Err(Error(ErrorKind::RetrievalError(e))) => {
                        if all {
                            return Err(Error::new(ErrorKind::RetrievalError(e)));
                        }
                    },

                    Err(e) => return Err(e)
                }
            }
            else {
                accumulated.push(each);
            }
        }
        Ok(accumulated)

    }


}

fn compare_values(first: &PathAwareValue, other: &PathAwareValue) -> Result<Ordering, Error> {
    match (first, other) {
        //
        // scalar values
        //
        (PathAwareValue::Null(_), PathAwareValue::Null(_)) => Ok(Ordering::Equal),
        (PathAwareValue::Int((_, i)), PathAwareValue::Int((_, o))) => Ok(i.cmp(o)),
        (PathAwareValue::String((_, s)), PathAwareValue::String((_, o))) => Ok(s.cmp(o)),
        (PathAwareValue::Float((_, f)), PathAwareValue::Float((_, s))) => match f.partial_cmp(s) {
            Some(o) => Ok(o),
            None => Err(Error::new(ErrorKind::NotComparable("Float values are not comparable".to_owned())))
        },
        (PathAwareValue::Char((_, f)), PathAwareValue::Char((_, s))) => Ok(f.cmp(s)),
        (PathAwareValue::Bool(_b), PathAwareValue::Bool(_b2)) => Ok(Ordering::Equal),
        (PathAwareValue::Regex(_r), PathAwareValue::Regex(_r2)) => Ok(Ordering::Equal),
        (_, _) => Err(Error::new(ErrorKind::NotComparable(
            format!("PathAwareValues are not comparable {}, {}", first.type_info(), other.type_info()))))
    }
}

pub(crate) fn compare_eq(first: &PathAwareValue, second: &PathAwareValue) -> Result<bool, Error> {
    let (reg, s) = match (first, second) {
        (PathAwareValue::String((_, s)), PathAwareValue::Regex((_, r))) => (regex::Regex::new(r.as_str())?, s.as_str()),
        (PathAwareValue::Regex((_, r)), PathAwareValue::String((_, s))) => (regex::Regex::new(r.as_str())?, s.as_str()),
        (_,_) => return Ok(first == second),
    };
    Ok(reg.is_match(s))
}

pub(crate) fn compare_lt(first: &PathAwareValue, other: &PathAwareValue) -> Result<bool, Error> {
    match compare_values(first, other) {
        Ok(o) => match o {
            Ordering::Equal | Ordering::Greater => Ok(false),
            Ordering::Less => Ok(true)
        },
        Err(e) => Err(e)
    }
}

pub(crate) fn compare_le(first: &PathAwareValue, other: &PathAwareValue) -> Result<bool, Error> {
    match compare_values(first, other) {
        Ok(o) => match o {
            Ordering::Greater => Ok(false),
            Ordering::Equal | Ordering::Less => Ok(true)
        },
        Err(e) => Err(e)
    }
}

pub(crate) fn compare_gt(first: &PathAwareValue, other: &PathAwareValue) -> Result<bool, Error> {
    match compare_values(first, other) {
        Ok(o) => match o {
            Ordering::Greater => Ok(true),
            Ordering::Less | Ordering::Equal => Ok(false)
        },
        Err(e) => Err(e)
    }
}

pub(crate) fn compare_ge(first: &PathAwareValue, other: &PathAwareValue) -> Result<bool, Error> {
    match compare_values(first, other) {
        Ok(o) => match o {
            Ordering::Greater | Ordering::Equal => Ok(true),
            Ordering::Less => Ok(false)
        },
        Err(e) => Err(e)
    }
}

#[cfg(test)]
#[path = "path_value_tests.rs"]
mod path_value_tests;
