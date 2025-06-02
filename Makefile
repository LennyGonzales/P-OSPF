IMAGE_NAME = muslgcc-builder

#################
# DEV section   #
#################

build: ## Build the docker image
	docker build -t $(IMAGE_NAME) .
.PHONY: build

run: ## Compile the .c file with the Makefile shared with the mounted volume
ifndef DIR
	$(error DIR is undefined. Please specify the directory using 'make run DIR=<directory>')
endif
	docker run --rm -v $(PWD)/src:/app -e TARGET=$(DIR) $(IMAGE_NAME)
.PHONY: run

clean: ## Remove docker image
	docker rmi $(IMAGE_NAME)
.PHONY: clean

#################
# Help Section  #
#################

all: help
.PHONY: all

help: ## List available commands
	@grep -E '^[0-9a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-30s\033[0m %s\n", $$1, $$2}'

.DEFAULT_GOAL := help
.PHONY: help
