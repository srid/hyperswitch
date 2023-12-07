use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};

use crate::{
    dense_map::DenseMap,
    error::GraphError,
    graph::ConstraintGraph,
    types::{
        DomainId, DomainIdentifier, DomainInfo, Edge, EdgeId, Metadata, Node, NodeId, NodeType,
        NodeValue, Relation, Strength, ValueNode,
    },
};

pub struct ConstraintGraphBuilder<'a, V: ValueNode> {
    domain: DenseMap<DomainId, DomainInfo<'a>>,
    nodes: DenseMap<NodeId, Node<V>>,
    edges: DenseMap<EdgeId, Edge>,
    domain_identifier_map: FxHashMap<DomainIdentifier<'a>, DomainId>,
    value_map: FxHashMap<NodeValue<V>, NodeId>,
    edges_map: FxHashMap<(NodeId, NodeId), EdgeId>,
    node_info: DenseMap<NodeId, Option<&'static str>>,
    node_metadata: DenseMap<NodeId, Option<Arc<dyn Metadata>>>,
}

#[allow(clippy::new_without_default)]
impl<'a, V> ConstraintGraphBuilder<'a, V>
where
    V: ValueNode,
{
    pub fn new() -> Self {
        Self {
            domain: DenseMap::new(),
            nodes: DenseMap::new(),
            edges: DenseMap::new(),
            domain_identifier_map: FxHashMap::default(),
            value_map: FxHashMap::default(),
            edges_map: FxHashMap::default(),
            node_info: DenseMap::new(),
            node_metadata: DenseMap::new(),
        }
    }

    pub fn build(self) -> ConstraintGraph<'a, V> {
        ConstraintGraph {
            domain: self.domain,
            nodes: self.nodes,
            edges: self.edges,
            value_map: self.value_map,
            node_info: self.node_info,
            node_metadata: self.node_metadata,
        }
    }

    pub fn make_domain(
        &mut self,
        domain_identifier: DomainIdentifier<'a>,
        domain_description: String,
    ) -> Result<DomainId, GraphError<V>> {
        Ok(self
            .domain_identifier_map
            .clone()
            .get(&domain_identifier)
            .map_or_else(
                || {
                    let domain_id = self.domain.push(DomainInfo {
                        domain_identifier: domain_identifier.clone(),
                        domain_description,
                    });
                    self.domain_identifier_map
                        .insert(domain_identifier.clone(), domain_id);
                    domain_id
                },
                |domain_id| *domain_id,
            ))
    }

    pub fn make_value_node<M: Metadata>(
        &mut self,
        value: NodeValue<V>,
        info: Option<&'static str>,
        domain_identifiers: Vec<DomainIdentifier<'_>>,
        metadata: Option<M>,
    ) -> Result<NodeId, GraphError<V>> {
        match self.value_map.get(&value).copied() {
            Some(node_id) => Ok(node_id),
            None => {
                let mut domain_ids: Vec<DomainId> = Vec::new();
                for domain_ident in &domain_identifiers {
                    domain_ids.push(
                        self.domain_identifier_map
                            .get(domain_ident)
                            .copied()
                            .ok_or(GraphError::DomainNotFound)?,
                    );
                }

                let node_id = self
                    .nodes
                    .push(Node::new(NodeType::Value(value.clone()), domain_ids));
                let _node_info_id = self.node_info.push(info);

                let _node_metadata_id = self
                    .node_metadata
                    .push(metadata.map(|meta| -> Arc<dyn Metadata> { Arc::new(meta) }));

                self.value_map.insert(value, node_id);
                Ok(node_id)
            }
        }
    }

    pub fn make_edge(
        &mut self,
        pred_id: NodeId,
        succ_id: NodeId,
        strength: Strength,
        relation: Relation,
    ) -> Result<EdgeId, GraphError<V>> {
        self.ensure_node_exists(pred_id)?;
        self.ensure_node_exists(succ_id)?;
        self.edges_map
            .get(&(pred_id, succ_id))
            .copied()
            .and_then(|edge_id| self.edges.get(edge_id).cloned().map(|edge| (edge_id, edge)))
            .map_or_else(
                || {
                    let edge_id = self.edges.push(Edge {
                        strength,
                        relation,
                        pred: pred_id,
                        succ: succ_id,
                    });
                    self.edges_map.insert((pred_id, succ_id), edge_id);

                    let pred = self
                        .nodes
                        .get_mut(pred_id)
                        .ok_or(GraphError::NodeNotFound)?;
                    pred.succs.push(edge_id);

                    let succ = self
                        .nodes
                        .get_mut(succ_id)
                        .ok_or(GraphError::NodeNotFound)?;
                    succ.preds.push(edge_id);

                    Ok(edge_id)
                },
                |(edge_id, edge)| {
                    if edge.strength == strength && edge.relation == relation {
                        Ok(edge_id)
                    } else {
                        Err(GraphError::ConflictingEdgeCreated)
                    }
                },
            )
    }

    pub fn make_all_aggregator<M: Metadata>(
        &mut self,
        nodes: &[(NodeId, Relation, Strength)],
        info: Option<&'static str>,
        metadata: Option<M>,
        domain: Vec<DomainIdentifier<'_>>,
    ) -> Result<NodeId, GraphError<V>> {
        nodes
            .iter()
            .try_for_each(|(node_id, _, _)| self.ensure_node_exists(*node_id))?;

        let mut domain_ids: Vec<DomainId> = Vec::new();
        for domain_ident in &domain {
            domain_ids.push(
                self.domain_identifier_map
                    .get(domain_ident)
                    .copied()
                    .ok_or(GraphError::DomainNotFound)?,
            );
        }

        let aggregator_id = self
            .nodes
            .push(Node::new(NodeType::AllAggregator, domain_ids));
        let _aggregator_info_id = self.node_info.push(info);

        let _node_metadata_id = self
            .node_metadata
            .push(metadata.map(|meta| -> Arc<dyn Metadata> { Arc::new(meta) }));

        for (node_id, relation, strength) in nodes {
            self.make_edge(*node_id, aggregator_id, *strength, *relation)?;
        }

        Ok(aggregator_id)
    }

    pub fn make_any_aggregator<M: Metadata>(
        &mut self,
        nodes: &[(NodeId, Relation)],
        info: Option<&'static str>,
        metadata: Option<M>,
        domain: Vec<DomainIdentifier<'_>>,
    ) -> Result<NodeId, GraphError<V>> {
        nodes
            .iter()
            .try_for_each(|(node_id, _)| self.ensure_node_exists(*node_id))?;

        let mut domain_ids: Vec<DomainId> = Vec::new();
        for domain_ident in &domain {
            domain_ids.push(
                self.domain_identifier_map
                    .get(domain_ident)
                    .copied()
                    .ok_or(GraphError::DomainNotFound)?,
            );
        }

        let aggregator_id = self
            .nodes
            .push(Node::new(NodeType::AnyAggregator, domain_ids));
        let _aggregator_info_id = self.node_info.push(info);

        let _node_metadata_id = self
            .node_metadata
            .push(metadata.map(|meta| -> Arc<dyn Metadata> { Arc::new(meta) }));

        for (node_id, relation) in nodes {
            self.make_edge(*node_id, aggregator_id, Strength::Strong, *relation)?;
        }

        Ok(aggregator_id)
    }

    pub fn make_in_aggregator<M: Metadata>(
        &mut self,
        values: Vec<V>,
        info: Option<&'static str>,
        metadata: Option<M>,
        domain: Vec<DomainIdentifier<'_>>,
    ) -> Result<NodeId, GraphError<V>> {
        let key = values
            .first()
            .ok_or(GraphError::NoInAggregatorValues)?
            .get_key();

        for val in &values {
            if val.get_key() != key {
                Err(GraphError::MalformedGraph {
                    reason: "Values for 'In' aggregator not of same key".to_string(),
                })?;
            }
        }

        let mut domain_ids: Vec<DomainId> = Vec::new();
        for domain_ident in &domain {
            domain_ids.push(
                self.domain_identifier_map
                    .get(domain_ident)
                    .copied()
                    .ok_or(GraphError::DomainNotFound)?,
            );
        }

        let node_id = self.nodes.push(Node::new(
            NodeType::InAggregator(FxHashSet::from_iter(values)),
            domain_ids,
        ));
        let _aggregator_info_id = self.node_info.push(info);

        let _node_metadata_id = self
            .node_metadata
            .push(metadata.map(|meta| -> Arc<dyn Metadata> { Arc::new(meta) }));

        Ok(node_id)
    }

    fn ensure_node_exists(&self, id: NodeId) -> Result<(), GraphError<V>> {
        if self.nodes.contains_key(id) {
            Ok(())
        } else {
            Err(GraphError::NodeNotFound)
        }
    }
}
