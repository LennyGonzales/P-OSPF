mod path_calculation {
    use std::collections::HashMap;

    #[derive(Debug, Clone)]
    struct Link {
        destination: String,
        cost: u32,
        capacity: u32,
        is_active: bool,
    }

    #[derive(Debug, Clone)]
    struct Router {
        id: String,
        links: HashMap<String, Link>,
    }

    impl Router {
        fn new(id: &str) -> Self {
            Router {
                id: id.to_string(),
                links: HashMap::new(),
            }
        }

        fn add_link(&mut self, destination: &str, cost: u32, capacity: u32, is_active: bool) {
            let link = Link {
                destination: destination.to_string(),
                cost,
                capacity,
                is_active,
            };
            self.links.insert(destination.to_string(), link);
        }

        fn calculate_best_paths(&self) -> HashMap<String, (u32, Vec<String>)> {
            let mut best_paths: HashMap<String, (u32, Vec<String>)> = HashMap::new();
            let mut visited = HashMap::new();
            let mut to_visit = vec![(self.id.clone(), 0, vec![self.id.clone()])];

            while let Some((current_id, current_cost, path)) = to_visit.pop() {
                if visited.contains_key(&current_id) {
                    continue;
                }
                visited.insert(current_id.clone(), current_cost);

                for link in self.links.values() {
                    if link.is_active {
                        let new_cost = current_cost + link.cost;
                        let mut new_path = path.clone();
                        new_path.push(link.destination.clone());

                        if !best_paths.contains_key(&link.destination) || new_cost < best_paths[&link.destination].0 {
                            best_paths.insert(link.destination.clone(), (new_cost, new_path));
                        }
                        to_visit.push((link.destination.clone(), new_cost, new_path));
                    }
                }
            }
            best_paths
        }
    }

    pub fn update_router(router: &mut Router, destination: &str, cost: u32, capacity: u32, is_active: bool) {
        router.add_link(destination, cost, capacity, is_active);
    }

    pub fn calculate_paths(router: &Router) -> HashMap<String, (u32, Vec<String>)> {
        router.calculate_best_paths()
    }
}